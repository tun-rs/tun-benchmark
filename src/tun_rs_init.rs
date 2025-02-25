use std::net::Ipv4Addr;
use std::sync::Arc;

use crate::*;
use bytes::BytesMut;
use pnet_packet::ip::IpNextHeaderProtocols;
use pnet_packet::ipv4::MutableIpv4Packet;
use pnet_packet::tcp::MutableTcpPacket;
use pnet_packet::MutablePacket;
use tun_rs::AsyncDevice;

pub fn start_sync() {
    let sync_device = tun_rs::DeviceBuilder::new()
        .ipv4(SRC_IP, 24, None)
        .mtu(1500)
        .build_sync()
        .unwrap();
    let mut buf = vec![0; 1500];
    loop {
        let len = match sync_device.recv(&mut buf) {
            Ok(len) => len,
            Err(e) => {
                log::error!("sync_device recv {e:?}");
                continue;
            }
        };
        if proxy(&mut buf[..len]).is_some() {
            if let Err(e) = sync_device.send(&buf[..len]) {
                log::error!("sync_device send {e:?}");
            }
        }
    }
}

pub async fn start_async() {
    let async_device = tun_rs::DeviceBuilder::new()
        .ipv4(SRC_IP, 24, None)
        .mtu(1500)
        .build_async()
        .unwrap();
    task(async_device).await;
}
pub async fn start_channel_async() {
    let async_device = tun_rs::DeviceBuilder::new()
        .ipv4(SRC_IP, 24, None)
        .mtu(1500)
        .build_async()
        .unwrap();
    let async_device = Arc::new(async_device);
    let async_device_send = async_device.clone();
    let mut buf = vec![0; 1500];
    let (s, mut r) = tokio::sync::mpsc::channel::<BytesMut>(128);
    tokio::spawn(async move {
        while let Some(buf) = r.recv().await {
            if let Err(e) = async_device_send.send(&buf).await {
                log::error!("async_device send {e:?}");
            }
        }
    });
    loop {
        let len = match async_device.recv(&mut buf).await {
            Ok(len) => len,
            Err(e) => {
                log::error!("async_device recv {e:?}");
                continue;
            }
        };
        if proxy(&mut buf[..len]).is_some() {
            let mut send_buf = BytesMut::with_capacity(1500);
            send_buf.extend_from_slice(&buf[..len]);
            s.send(send_buf).await.unwrap();
        }
    }
}

#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub async fn start_multi_queue_async() {
    let async_device = tun_rs::DeviceBuilder::new()
        .ipv4(SRC_IP, 24, None)
        .mtu(1500)
        .multi_queue(true)
        .build_async()
        .unwrap();
    let async_device_clone = async_device.try_clone().unwrap();
    let handle1 = tokio::spawn(task(async_device));
    let handle2 = tokio::spawn(task(async_device_clone));
    _ = tokio::join!(handle1, handle2);
}
async fn task(async_device: AsyncDevice) {
    let mut buf = vec![0; 1500];
    loop {
        let len = match async_device.recv(&mut buf).await {
            Ok(len) => len,
            Err(e) => {
                log::error!("async_device recv {e:?}");
                continue;
            }
        };
        if proxy(&mut buf[..len]).is_some() {
            if let Err(e) = async_device.send(&buf[..len]).await {
                log::error!("async_device send {e:?}");
            }
        }
    }
}
#[cfg(target_os = "linux")]
pub async fn start_tso_async() {
    let async_device = tun_rs::DeviceBuilder::new()
        .ipv4(SRC_IP, 24, None)
        .mtu(1500)
        .offload(true)
        .build_async()
        .unwrap();
    let async_device = Arc::new(async_device);
    println!(
        "TCP-GSO:{},UDP-GSO:{}",
        async_device.tcp_gso(),
        async_device.udp_gso()
    );
    let mut original_buffer = vec![0; tun_rs::VIRTIO_NET_HDR_LEN + 65535];
    let mut bufs = vec![BytesMut::zeroed(1500); tun_rs::IDEAL_BATCH_SIZE];
    let mut sizes = vec![0; tun_rs::IDEAL_BATCH_SIZE];
    let mut gro_table = tun_rs::GROTable::default();
    let mut send_buf = BytesMut::with_capacity(tun_rs::VIRTIO_NET_HDR_LEN + 65535);
    send_buf.resize(tun_rs::VIRTIO_NET_HDR_LEN, 0);
    loop {
        let num = match async_device
            .recv_multiple(&mut original_buffer, &mut bufs, &mut sizes, 0)
            .await
        {
            Ok(len) => len,
            Err(e) => {
                log::error!("async_device recv {e:?}");
                continue;
            }
        };
        for i in 0..num {
            let buf = &mut bufs[i][..sizes[i]];
            if proxy(buf).is_some() {
                send_buf.truncate(tun_rs::VIRTIO_NET_HDR_LEN);
                send_buf.extend_from_slice(buf);
                if let Err(e) = async_device
                    .send_multiple(
                        &mut gro_table,
                        &mut [&mut send_buf],
                        tun_rs::VIRTIO_NET_HDR_LEN,
                    )
                    .await
                {
                    log::error!("async_device send {e:?}");
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub async fn start_tso_channel_async() {
    let async_device = tun_rs::DeviceBuilder::new()
        .ipv4(SRC_IP, 24, None)
        .mtu(1500)
        .offload(true)
        .build_async()
        .unwrap();
    let async_device = Arc::new(async_device);
    let async_device_send = async_device.clone();
    println!(
        "TCP-GSO:{},UDP-GSO:{}",
        async_device.tcp_gso(),
        async_device.udp_gso()
    );
    let mut original_buffer = vec![0; tun_rs::VIRTIO_NET_HDR_LEN + 65535];
    let mut bufs = vec![BytesMut::zeroed(1500); tun_rs::IDEAL_BATCH_SIZE];
    let mut sizes = vec![0; tun_rs::IDEAL_BATCH_SIZE];
    let mut gro_table = tun_rs::GROTable::default();

    let (s, mut r) = tokio::sync::mpsc::channel::<BytesMut>(128);
    tokio::spawn(async move {
        let mut list = Vec::new();
        while let Some(buf) = r.recv().await {
            list.clear();
            list.push(buf);
            while let Ok(buf) = r.try_recv() {
                list.push(buf)
            }
            if let Err(e) = async_device_send
                .send_multiple(&mut gro_table, &mut list, tun_rs::VIRTIO_NET_HDR_LEN)
                .await
            {
                log::error!("async_device send {e:?}");
            }
        }
    });
    loop {
        let num = match async_device
            .recv_multiple(&mut original_buffer, &mut bufs, &mut sizes, 0)
            .await
        {
            Ok(len) => len,
            Err(e) => {
                log::error!("async_device recv {e:?}");
                continue;
            }
        };
        for i in 0..num {
            let buf = &mut bufs[i][..sizes[i]];
            if proxy(buf).is_some() {
                let mut send_buf = BytesMut::with_capacity(tun_rs::VIRTIO_NET_HDR_LEN + 65535);
                send_buf.resize(tun_rs::VIRTIO_NET_HDR_LEN, 0);
                send_buf.extend_from_slice(buf);
                s.send(send_buf).await.unwrap();
            }
        }
    }
}

fn proxy(buf: &mut [u8]) -> Option<()> {
    let mut ip_packet = MutableIpv4Packet::new(buf)?;
    if ip_packet.get_source() != SRC_IP {
        return None;
    }
    if ip_packet.get_next_level_protocol() != IpNextHeaderProtocols::Tcp {
        return None;
    }
    if ip_packet.get_destination() == DST_IP_A {
        return proxy_dst(&mut ip_packet, DST_IP_B);
    }
    if ip_packet.get_destination() == DST_IP_B {
        return proxy_dst(&mut ip_packet, DST_IP_A);
    }
    None
}
fn proxy_dst(ip_packet: &mut MutableIpv4Packet, dst: Ipv4Addr) -> Option<()> {
    let mut tcp_packet = MutableTcpPacket::new(ip_packet.payload_mut())?;

    let check_sum = pnet_packet::tcp::ipv4_checksum(&tcp_packet.to_immutable(), &dst, &SRC_IP);

    tcp_packet.set_checksum(check_sum);

    ip_packet.set_destination(SRC_IP);
    ip_packet.set_source(dst);
    let check_sum = pnet_packet::ipv4::checksum(&ip_packet.to_immutable());
    ip_packet.set_checksum(check_sum);
    Some(())
}
