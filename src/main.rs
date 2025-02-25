use std::fmt::{Debug, Formatter};
use std::net::Ipv4Addr;
use std::thread;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::Receiver;

const SRC_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 1, 2);
const DST_IP_A: Ipv4Addr = Ipv4Addr::new(10, 0, 1, 3);
const DST_IP_B: Ipv4Addr = Ipv4Addr::new(10, 0, 1, 4);
const TEST_DATA_SIZE: usize = 1000 * 1024 * 1024; // 1000MB
mod tun_rs_init;
#[derive(Copy, Clone)]
pub enum TunType {
    TunRsStandard,
    TunRsChannelStandard,
    TunRsSyncStandard,
    #[cfg(target_os = "linux")]
    TunRsTSO,
    #[cfg(target_os = "linux")]
    TunRsChannelTSO,
    #[cfg(target_os = "linux")]
    TunRsMultiQueue,
}
impl Debug for TunType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            TunType::TunRsStandard => "tun-rs: standard, recv/send",
            TunType::TunRsChannelStandard => "tun-rs: use channel, recv/send",
            TunType::TunRsSyncStandard => "tun-rs: sync-standard, recv/send",
            #[cfg(target_os = "linux")]
            TunType::TunRsTSO => "tun-rs: use tso, recv_multiple/send_multiple",
            #[cfg(target_os = "linux")]
            TunType::TunRsChannelTSO => "tun-rs: use tso+channel, recv_multiple/send_multiple",
            #[cfg(target_os = "linux")]
            TunType::TunRsMultiQueue => "tun-rs: use multi-queue, recv/send",
        };
        f.write_str(str)
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    benchmark(3, TunType::TunRsStandard).await;
    benchmark(3, TunType::TunRsChannelStandard).await;
    #[cfg(target_os = "linux")]
    benchmark(3, TunType::TunRsTSO).await;
    #[cfg(target_os = "linux")]
    benchmark(3, TunType::TunRsChannelTSO).await;
    #[cfg(target_os = "linux")]
    benchmark(3, TunType::TunRsMultiQueue).await;
    // Because the synchronization did not close Tun,
    // it can only be written at the end and executed once
    benchmark(1, TunType::TunRsSyncStandard).await;
}
pub async fn benchmark(n: usize, tun_type: TunType) {
    let (s, r) = tokio::sync::mpsc::channel::<()>(1);
    println!("start {tun_type:?}");
    start_tun(tun_type, r);
    tokio::time::sleep(Duration::from_secs(5)).await;
    test_batch(n, format!("{tun_type:?}"));
    s.send(()).await.unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
}

pub fn start_tun(tun_type: TunType, r: Receiver<()>) {
    thread::spawn(move || {
        tun_main(tun_type, r);
    });
}
#[tokio::main]
pub async fn tun_main(tun_type: TunType, mut r: Receiver<()>) {
    tokio::spawn(async move {
        match tun_type {
            TunType::TunRsStandard => {
                Box::pin(tun_rs_init::start_async()).await;
            }
            TunType::TunRsChannelStandard => {
                Box::pin(tun_rs_init::start_channel_async()).await;
            }
            TunType::TunRsSyncStandard => {
                // It shouldn't be done this way, but it's not wrong
                thread::spawn(move || {
                    tun_rs_init::start_sync();
                });
            }
            #[cfg(target_os = "linux")]
            TunType::TunRsTSO => {
                Box::pin(tun_rs_init::start_tso_async()).await;
            }
            #[cfg(target_os = "linux")]
            TunType::TunRsChannelTSO => {
                Box::pin(tun_rs_init::start_tso_channel_async()).await;
            }
            #[cfg(target_os = "linux")]
            TunType::TunRsMultiQueue => {
                Box::pin(tun_rs_init::start_multi_queue_async()).await;
            }
        }
    });
    r.recv().await;
    println!("end {tun_type:?}");
}

pub fn test_batch(n: usize, name: String) {
    for _ in 0..n {
        println!("===================== {name} =====================");
        thread::spawn(move || {
            test_main();
        })
        .join()
        .unwrap();
    }
}
#[tokio::main]
pub async fn test_main() {
    tokio::time::sleep(Duration::from_secs(2)).await;
    speed_test().await;
}

pub async fn speed_test() {
    let tcp_listener = TcpListener::bind(format!("{SRC_IP}:8080")).await.unwrap();
    let (s, mut r) = tokio::sync::mpsc::channel::<()>(1);
    let h = tokio::spawn(async move {
        loop {
            let (mut stream, addr) = tcp_listener.accept().await.unwrap();
            log::info!("accept {addr}");
            let s = s.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 8192]; // 8KB
                let mut total_received = 0;
                let start_time = Instant::now();

                while let Ok(n) = stream.read(&mut buffer).await {
                    if n == 0 {
                        break;
                    }
                    total_received += n;
                }

                let duration = start_time.elapsed();
                let speed = total_received as f64 / duration.as_secs_f64() / (1024.0 * 1024.0);

                println!(
                    "Received: {} MB in {:.2?}, Speed: {:.2} MB/s",
                    total_received / (1024 * 1024),
                    duration,
                    speed
                );
                s.send(()).await.unwrap();
            });
        }
    });
    let mut tcp_stream = TcpStream::connect(format!("{DST_IP_A}:8080"))
        .await
        .unwrap();
    let data = vec![0u8; 8192]; // 8KB
    let mut sent_bytes = 0;
    let start_time = Instant::now();

    while sent_bytes < TEST_DATA_SIZE {
        let n = tcp_stream.write(&data).await.unwrap();
        sent_bytes += n;
    }
    drop(tcp_stream);
    let duration = start_time.elapsed();
    let speed = sent_bytes as f64 / duration.as_secs_f64() / (1024.0 * 1024.0);

    println!(
        "Sent: {} MB in {:.2?}, Speed: {:.2} MB/s",
        sent_bytes / (1024 * 1024),
        duration,
        speed
    );
    r.recv().await.unwrap();
    h.abort();
}
