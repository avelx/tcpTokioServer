use std::error::Error;
use std::{io, thread, time};
use std::str;
use crossbeam_channel::{bounded, Receiver, SendError, Sender, TryRecvError};
use tokio::io::{AsyncReadExt, Interest};
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use log::{info, error, warn};

use simplelog::*;

use std::fs::File;

const REMOTE_RESOURCE: &str = "216.58.204.78:80";

async fn remote_server_thread(
    tx_a: Sender<String>,
    rx_a: Receiver<String>,
) -> Result<(), Box<dyn Error>> {
    info!("Setup remote connection thread ...");

    let std_stream = std::net::TcpStream::connect(REMOTE_RESOURCE)?;
    std_stream.set_nonblocking(true)?;
    let mut remote_stream = TcpStream::from_std(std_stream).unwrap();

    loop {
        let ready = remote_stream
            .ready(Interest::READABLE | Interest::WRITABLE)
            .await
            .unwrap();

        info!("Remote->Stream status: {:?}", ready);
        //let ten_millis = time::Duration::from_millis(2000);
        //thread::sleep(ten_millis);

        if ready.is_readable() {
            info!("Remote->Ready to read ...");
            let mut data = vec![0; 8024];
            // Try to read data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.
            match remote_stream.try_read(&mut data) {
                Ok(_) => {
                    let response_string = str::from_utf8(&data).unwrap();
                    info!("Remote->Response2:\n\r{}", response_string);
                    //let val = String::from("hi");
                    tx_a.send(response_string.to_string()).unwrap();
                    return Ok(());
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    error!("Remote->Read2: {}", e);
                    continue;
                }
                Err(e) => {
                    error!("Remote->Read2: {}", e);
                    return Err(e.into());
                }
            }
        }


        if ready.is_writable() {
            info!("Writable ...");
            while let Ok(request_remote) = rx_a.try_recv() {
                let request_remote2: String = request_remote
                    .replace("Host: localhost:8181", "Host: google.com");
                //.replace("GET /", "GET http://google.com");
                //.add("\r\n");
                //.trim().to_string();
                info!("Request->Remote/modified:\n\r{}", request_remote2);

                // Try to write data, this may still fail with `WouldBlock`
                // if the readiness event is a false positive.
                if request_remote2.len() > 0 {
                    info!("Attempt to write");
                    match remote_stream.try_write(request_remote2.as_bytes()) {
                        Ok(n) => {
                            info!("Remote write {} bytes", n);
                            //remoteStream.flush().await.unwrap();
                            //remoteStream.shutdown(); //.expect("shutdown call failed")

                            // Try to read data, this may still fail with `WouldBlock`
                            // if the readiness event is a false positive.
                            let mut response: String = String::from("");
                            remote_stream.read_to_string(&mut response).await.unwrap();
                            //let response_string = str::from_utf8(&data).unwrap();
                            info!("Remote->Response:\n\r{} - {}", response, response.len());
                            if response.len() > 0 {
                                let res: Result<(), SendError<String>> = tx_a.send(response.to_string());
                                //txA.send(response.to_string()).unwrap_err();
                                match res {
                                    Ok(e) => {
                                        error!("Error sending response: {:?}", e);
                                        continue;
                                    }
                                    // SendError(e: String) => {
                                    //     error!("Error sending response: {:?}", e);
                                    //     continue;
                                    // },
                                    _ => {
                                        error!("Error sending response v6");
                                        continue
                                    }
                                }
                            }

                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            error!("Remote write error: {}", e);
                            continue
                        }
                        Err(e) => {
                            error!("Remote write error: {}", e);
                            return Err(e.into());
                        }
                    }
                }
            }
        }
        //continue;
    }
}

async fn process_local_stream(local_stream: TcpStream) -> Result<(), Box<dyn Error>> {

    //Transmitter from local / Receiver to remote
    let (tx_local, rx_remote) = bounded(0);
    // Transmitter from remote, Receiver to local
    let (tx_remote, rx_local) = bounded(0);

    tokio::spawn(async {
        let res = remote_server_thread(tx_remote, rx_remote).await;
        match res {
            Ok(_) => {
                info!("Local->thread result: OK");
            }
            Err(ref e) => {
                error!("Local->thread result: ERROR: {}", e);
            }
        }
    });
    // let handler = thread::spawn(move || {remote_server_thread(txRemote, rxRemote) } );
    // handler.join().unwrap();

    info!("Local:Processing ...");
    loop {
        let ready = local_stream
            .ready(Interest::READABLE | Interest::WRITABLE)
            .await
            .unwrap();

        info!("Local->Stream status: {:?}", ready);

        if ready.is_readable() {
            let mut data = vec![0; 1024];
            // Try to read data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.

            match local_stream.try_read(&mut data) {
                Ok(_) => {
                    let response_string = str::from_utf8(&data).unwrap();
                    info!("Local->Response: {}", response_string);
                    //let val = String::from("hi");
                    tx_local.send(response_string.to_string()).unwrap();
                    continue;
                    //return Ok(());
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    warn!("Local->Read error: {}", e);
                    continue;
                }
                Err(e) => {
                    error!("Local->Read error/v2: {}", e);
                    return Err(e.into());
                }
            }
        }

        if ready.is_writable() {
            info!("Local->Read ...");
            let request_local: Result<String, TryRecvError> = rx_local.try_recv();
            //info!("Local->Request bytes: {}", request_local);
            // Try to write data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.
            match request_local {
                Ok(request) => {
                    match local_stream.try_write(request.as_ref()) {
                        Ok(n) => {
                            info!("Local=>write {} bytes", n);
                            continue
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            error!("Local->Write error: {}", e);
                            continue
                        },
                        Err(e) => {
                            error!("Local->Write error/v2: {}", e);
                            return Err(e.into());
                        }
                    }
                }
                Err(e) => {
                    warn!("Local->Write error/v3: {}", e);
                    let ten_millis = time::Duration::from_millis(25);
                    thread::sleep(ten_millis);
                    continue;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {

    CombinedLogger::init(
        vec![
            TermLogger::new(LevelFilter::Error, Config::default(), TerminalMode::Mixed, ColorChoice::Auto),
            WriteLogger::new(LevelFilter::Info, Config::default(), File::create("logs/tcpTokioServer.log").unwrap()),
        ]
    ).unwrap();

    let local_listener = TcpListener::bind("127.0.0.1:8181").await?;
    loop {
        let (socket, _) = local_listener.accept().await?;
        info!("Ready to accept connection ...");
        process_local_stream(socket).await.expect("Filed to process incoming connection");
    }

}
