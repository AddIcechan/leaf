use std::{env, thread, net::TcpStream, io::{Write, Read}, time::Duration};
use tokio;


fn main(){
    
    let config = r#"
    {
        "log": {
            "level": "debug"
        },
        "dns": {
            "servers": [
                "114.114.114.114",
                "1.1.1.1",
                "8.8.8.8"
            ]
        },
        "inbounds": [
            {
                "protocol": "socks",
                "address": "127.0.0.1",
                "port": 7778
            },
            {
                "protocol": "tun",
                "address": "0.0.0.0",
                "port": 9998,
                "settings": {
                    "fd": 1,
                    "fakeDnsInclude": [
                        "google.com",
                        "gstatic.com"
                    ]
                }
            }
        ],
        "outbounds": [
            {
                "protocol": "failover",
                "settings": {
                    "actors": [
                        "ss1"
                    ],
                    "failTimeout":2,
                    "healthCheck":true,
                    "healthCheckTimeout":5,
                    "checkInterval":3,
                    "healthCheckActive":3,
                    "healthCheckAddr":"captive.apple.com:80",
                    "healthCheckContent":"HEAD / HTTP/1.1\r\n\r\n",
                    "failover":true
                },
                "tag": "failover_out"
            },
            {
                "protocol": "shadowsocks",
                "settings": {
                    "address": "128.1.62.188",
            "port": 40215,
            "password": "FX9PTCnvyu2CndLr",
            "method": "aes-256-gcm"
                },
                "tag":"ss"
            },
            {
                "protocol": "shadowsocks",
                "settings": {
                    "address": "127.0.0.1",
            "port": 6669,
            "password": "111111",
            "method": "aes-256-gcm"
                },
                "tag":"ss1"
            },
            {
                "protocol": "direct",
                "tag": "direct"
            }
        ],
        "router":{
            "domainResolve": true ,
            "rules": [
                {
                    "domainKeyword": [
                        "ipinfo",
                        "google",
                        "gstatic"
                    ],
                    "target": "direct"
                }
            ]
        }
        
    }
    "#;

    // thread::spawn(||{
        

    //     loop {
    //         thread::sleep(Duration::from_secs(1));
    //         let mut stream = TcpStream::connect("127.0.0.1:5555").expect("connect failed");
    //         stream
    //             .write(b"ping")
    //             .expect("write failed");
    //         let mut buf = *b"1111";
    //         stream.read_exact(&mut buf);
    //         println!("{:?}",String::from_utf8(buf.to_vec()));
    
    //     }
    // });
    let opts = leaf::StartOptions {
        config: leaf::Config::Str(config.to_string()),
        auto_reload: false,
        runtime_opt: leaf::RuntimeOption::MultiThreadAuto(2097152),
    };
    if let Err(e) = leaf::start(0, opts) {
        panic!("{}",e);
    }


    print!("end")

}