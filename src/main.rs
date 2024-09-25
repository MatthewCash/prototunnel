use anyhow::{bail, Result};
use clap::Parser;
use ipnet::Ipv4Net;
use std::{net::SocketAddr, os::fd::AsRawFd};
use tokio::{
    io::{split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task, try_join,
};
use tokio_tun::Tun;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    address: Ipv4Net,

    #[arg(short, long, default_value = "prototun")]
    name: String,

    #[arg(short, long, conflicts_with = "client")]
    server: Option<SocketAddr>,

    #[arg(short, long, conflicts_with = "server")]
    client: Option<SocketAddr>,

    #[arg(short, long, default_value_t = 1500)]
    mtu: u16,
}

async fn pipe(
    mut reader: impl AsyncRead + Unpin,
    mut writer: impl AsyncWrite + Unpin,
    mtu: u16,
) -> Result<()> {
    let mut buf = vec![0u8; mtu as usize + 4]; // 4 bytes for packet info

    loop {
        let read_bytes = reader.read(&mut buf).await?;
        let sent_bytes = writer.write(&buf[..read_bytes]).await?;
        if read_bytes != sent_bytes {
            bail!("Read {read_bytes} bytes but sent {sent_bytes} bytes!");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let tun = Tun::builder()
        .name(&args.name)
        .packet_info()
        .address(args.address.addr())
        .netmask(args.address.netmask())
        .mtu(args.mtu as i32)
        .up()
        .try_build()?;

    println!("tun created, name: {}, fd: {}", tun.name(), tun.as_raw_fd());

    let tcp_stream = if let Some(host) = args.server {
        TcpListener::bind(host).await?.accept().await?.0
    } else if let Some(host) = args.client {
        TcpStream::connect(host).await?
    } else {
        bail!("Either client or server operation must be specified!");
    };

    let (sock_reader, sock_writer) = split(tcp_stream);
    let (tun_reader, tun_writer) = split(tun);

    try_join!(
        task::spawn(pipe(sock_reader, tun_writer, args.mtu)),
        task::spawn(pipe(tun_reader, sock_writer, args.mtu))
    )?;

    println!("done?");

    Ok(())
}
