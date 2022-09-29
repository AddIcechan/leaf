use std::str::FromStr;
use std::{sync::Arc, time::Duration};

use log::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::from_str;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::{timeout, Instant};
use trust_dns_proto::{
    op::{header::MessageType, op_code::OpCode, query::Query, Message},
    rr::{record_type::RecordType, Name},
};

use crate::{app::SyncDnsClient, proxy::*, session::*};

pub mod datagram;
pub mod stream;

pub use datagram::Handler as DatagramHandler;
pub use stream::Handler as StreamHandler;

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(self) struct Measure(usize, u128); // (index, duration in millis)

pub(self) async fn health_check(
    network: Network,
    idx: usize,
    h: AnyOutboundHandler,
    dns_client: SyncDnsClient,
    delay: Duration,
    health_check_timeout: u32,
    health_check_addr:String,
    health_check_content:String
) -> Measure {
    tokio::time::sleep(delay).await;

    debug!(
        "health checking [{}] ({}) index ({})",
        h.tag(),
        &network,
        idx
    );
    let mut host = "www.google.com".to_string();
    let mut port = 80;
    let mut content  = "HEAD / HTTP/1.1\r\n\r\n".to_string();
    let arr = health_check_addr.split(":").collect::<Vec<&str>>();
    let mut dnsServer = "8.8.8.8".to_string();
    if arr.len() > 1{
        host = arr[0].to_string();
        port = match from_str(arr[1]){
            Ok(s) => s,
            Err(_) => port,
        };
        if arr.len() > 2{
            dnsServer = arr[2].to_string()
        }
    }
    let dns_addr = dnsServer.split(".").collect::<Vec<&str>>();
    let mut dns_socket_addr = Ipv4Addr::new(1, 1, 1, 1);
    if dns_addr.len() > 3{
        let a0:u8 = match from_str(dns_addr[0]){
            Ok(s) => s,
            Err(_) => 114,
        };
        let a1:u8 = match from_str(dns_addr[1]){
            Ok(s) => s,
            Err(_) => 114,
        };
        let a2:u8 = match from_str(dns_addr[2]){
            Ok(s) => s,
            Err(_) => 114,
        };
        let a3:u8 = match from_str(dns_addr[3]){
            Ok(s) => s,
            Err(_) => 114,
        };
        dns_socket_addr = Ipv4Addr::new(a0, a1, a2, a3);
    }
    content = match health_check_content.is_empty() {
        true => content,
        false => health_check_content,
    };
    let hostName = Name::from_str(&format!("{}.",host));

    let measure = async move {
        let dest = match network {
            Network::Tcp => SocksAddr::Domain(host, port),
            Network::Udp => {
                SocksAddr::Ip(SocketAddr::new(IpAddr::V4(dns_socket_addr), 53))
            }
        };

        let sess = Session {
            destination: dest,
            new_conn_once: true,
            ..Default::default()
        };

        let start = Instant::now();

        match network {
            Network::Tcp => {
                let stream =
                    match crate::proxy::connect_stream_outbound(&sess, dns_client, &h).await {
                        Ok(s) => s,
                        Err(_) => return Measure(idx, u128::MAX),
                    };
                let m: Measure;
                let h = if let Ok(h) = h.stream() {
                    h
                } else {
                    return Measure(idx, u128::MAX);
                };
                match h.handle(&sess, stream).await {
                    Ok(mut stream) => {
                        if stream.write_all(content.as_bytes()).await.is_err() {
                            return Measure(idx, u128::MAX - 2);
                        }
                        let mut buf = vec![0u8; 1];
                        match stream.read_exact(&mut buf).await {
                            Ok(_) => {
                                let elapsed = Instant::now().duration_since(start);
                                m = Measure(idx, elapsed.as_millis());
                            }
                            Err(_) => {
                                m = Measure(idx, u128::MAX - 3);
                            }
                        }
                        let _ = stream.shutdown().await;
                    }
                    Err(_) => {
                        m = Measure(idx, u128::MAX);
                    }
                }
                return m;
            }
            Network::Udp => {
                let transport =
                    match crate::proxy::connect_datagram_outbound(&sess, dns_client, &h).await {
                        Ok(t) => t,
                        Err(_) => return Measure(idx, u128::MAX),
                    };
                let h = if let Ok(h) = h.datagram() {
                    h
                } else {
                    return Measure(idx, u128::MAX);
                };
                match h.handle(&sess, transport).await {
                    Ok(socket) => {
                        let addr = SocksAddr::Ip(SocketAddr::new(
                            IpAddr::V4(dns_socket_addr),
                            53,
                        ));
                        let mut msg = Message::new();
                        let name = match hostName {
                            Ok(n) => n,
                            Err(e) => {
                                warn!("invalid domain name: {}", e);
                                return Measure(idx, u128::MAX);
                            }
                        };
                        let query = Query::query(name, RecordType::A);
                        msg.add_query(query);
                        let mut rng = StdRng::from_entropy();
                        let id: u16 = rng.gen();
                        msg.set_id(id);
                        msg.set_op_code(OpCode::Query);
                        msg.set_message_type(MessageType::Query);
                        msg.set_recursion_desired(true);
                        let msg_buf = match msg.to_vec() {
                            Ok(b) => b,
                            Err(e) => {
                                warn!("encode message to buffer failed: {}", e);
                                return Measure(idx, u128::MAX);
                            }
                        };

                        let (mut recv, mut send) = socket.split();

                        if send.send_to(&msg_buf, &addr).await.is_err() {
                            return Measure(idx, u128::MAX - 2);
                        }
                        let mut buf = [0u8; 1500];
                        match recv.recv_from(&mut buf).await {
                            Ok(_) => {
                                let elapsed = tokio::time::Instant::now().duration_since(start);
                                Measure(idx, elapsed.as_millis())
                            }
                            Err(_) => Measure(idx, u128::MAX - 3),
                        }
                    }
                    Err(_) => Measure(idx, u128::MAX),
                }
            }
        }
    };

    timeout(Duration::from_secs(health_check_timeout.into()), measure)
        .await
        .unwrap_or(Measure(idx, u128::MAX - 1))
}

pub(self) async fn health_check_task(
    network: Network,
    schedule: Arc<Mutex<Vec<usize>>>,
    actors: Vec<AnyOutboundHandler>,
    dns_client: SyncDnsClient,
    check_interval: u32,
    failover: bool,
    last_resort: Option<AnyOutboundHandler>,
    health_check_timeout: u32,
    health_check_delay: u32,
    health_check_active: u32,
    last_active: Arc<Mutex<Instant>>,
    health_check_addr:String,
    health_check_content:String,
) {
    loop {
        let last_active = Instant::now()
            .duration_since(*last_active.lock().await)
            .as_secs();

        if last_active < health_check_active.into() {
            let mut checks = Vec::new();
            let mut rng = StdRng::from_entropy();
            for (i, a) in (&actors).iter().enumerate() {
                let dns_client_cloned = dns_client.clone();
                let delay = Duration::from_millis(rng.gen_range(0..=health_check_delay) as u64);
                checks.push(Box::pin(health_check(
                    network,
                    i,
                    a.clone(),
                    dns_client_cloned,
                    delay,
                    health_check_timeout,
                    health_check_addr.clone(),
                    health_check_content.clone()
                )));
            }
            let mut measures = futures::future::join_all(checks).await;

            measures.sort_by(|a, b| a.1.cmp(&b.1));
            trace!("sorted health check results:\n{:#?}", measures);

            let priorities: Vec<String> = measures
                .iter()
                .map(|m| {
                    let mut repr = actors[m.0].tag().to_owned();
                    repr.push('(');
                    repr.push_str(m.1.to_string().as_str());
                    repr.push(')');
                    repr
                })
                .collect();

            debug!("priority after health check: {}", priorities.join(" > "));

            let mut schedule = schedule.lock().await;
            schedule.clear();

            let all_failed = |measures: &Vec<Measure>| -> bool {
                let threshold = Duration::from_secs(health_check_timeout.into()).as_millis();
                for m in measures.iter() {
                    if m.1 < threshold {
                        return false;
                    }
                }
                true
            };

            if !(last_resort.is_some() && all_failed(&measures)) {
                if !failover {
                    // if failover is disabled, put only 1 actor in schedule
                    schedule.push(measures[0].0);
                    trace!("put {} in schedule", measures[0].0);
                } else {
                    for m in measures {
                        schedule.push(m.0);
                        trace!("put {} in schedule", m.0);
                    }
                }
            }

            drop(schedule); // release
        } else {
            debug!("skip health check as no activities in {}s", last_active);
        }

        tokio::time::sleep(Duration::from_secs(check_interval as u64)).await;
    }
}
