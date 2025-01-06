use crossbeam_channel::{bounded, Receiver, SendError, Sender, TryRecvError};
use log::{error, info, warn};
use std::error::Error;
use std::str;
use std::{io, thread, time};
use tokio::io::{AsyncReadExt, Interest};
use tokio::net::TcpListener;
use tokio::net::TcpStream;

use simplelog::*;

use std::fs::File;
use std::io::Read;

use regex::{Match, Regex};
use std::fmt;

const REMOTE_RESOURCE: &str = "142.250.180.14:80";
const LOCAL_RESOURCE: &str = "127.0.0.1:8181";

// PARSING FUNCTION's:

// Parse Http response string and extract Location;
// Return first found location in the response or None if nothing found;
fn extract_location(response: String) -> Option<String> {
    let re: Regex = Regex::new(r"Location:\shttp://([a-zA-Z\\.]+)+").unwrap();
    let Some(location) = re.captures(response.as_str()) else {
        return None;
    };
    Some(location.get(1).unwrap().as_str().to_string())
}

// ASYNC FUNCTIONS
async fn remote_server_thread(
    tx_a: Sender<String>,
    rx_a: Receiver<String>,
) -> Result<(), Box<dyn Error>> {
    let std_stream = std::net::TcpStream::connect(REMOTE_RESOURCE)?;
    std_stream.set_nonblocking(true)?;
    let mut remote_stream = TcpStream::from_std(std_stream).unwrap();

    loop {
        let ready = remote_stream
            .ready(Interest::READABLE | Interest::WRITABLE)
            .await
            .unwrap();

        //info!("Remote->Stream status: {:?}", ready);

        if ready.is_readable() && !ready.is_read_closed() {
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

        if ready.is_writable() && !ready.is_write_closed() {
            while let Ok(request_remote) = rx_a.try_recv() {
                let request_remote2: String =
                    request_remote.replace("Host: localhost:8181", "Host: google.com");
                info!("Request->Remote/modified:\n\r{}", request_remote2);

                // Try to write data, this may still fail with `WouldBlock`
                // if the readiness event is a false positive.
                if request_remote2.len() > 0 {
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
                                let res: Result<(), SendError<String>> =
                                    tx_a.send(response.to_string());
                                match res {
                                    Ok(e) => {
                                        info!("Remote->sent response to local: {:?}", e);
                                        continue;
                                    }
                                    _ => {
                                        error!("Error sending response v6");
                                        continue;
                                    }
                                }
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            error!("Remote write error: {}", e);
                            continue;
                        }
                        Err(e) => {
                            error!("Remote write error: {}", e);
                            return Err(e.into());
                        }
                    }
                }
            }
        }
    }
}

async fn process_local_stream(local_stream: TcpStream) -> Result<String, Box<dyn Error>> {
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

    let mut max_attempts_to_write: i32 = 10;
    let mut final_response: String = String::from("");

    loop {
        let ready = local_stream
            .ready(Interest::READABLE | Interest::WRITABLE)
            .await
            .unwrap();

        info!("Local->Stream status: {:?}", ready);

        if (ready.is_readable() && !ready.is_read_closed()) {
            let mut data = vec![0; 1024];

            match local_stream.try_read(&mut data) {
                Ok(_) => {
                    let response_string = str::from_utf8(&data).unwrap();
                    info!("Local->Response: {}", response_string);

                    let sent_result = tx_local.send(response_string.to_string());
                    match sent_result {
                        Ok(_) => {
                            continue;
                        }
                        Err(e) => {
                            continue;
                        }
                    }
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

        if ready.is_writable() && !ready.is_write_closed() {
            info!("Local->Write ...");
            let request_local: Result<String, TryRecvError> = rx_local.try_recv();
            match request_local {
                Ok(request) => {
                    match local_stream.try_write(request.as_ref()) {
                        Ok(n) => {
                            // RETURN WHOLE RESPONSE BUDDY TO CALLING FUNCTION
                            let location = extract_location(request.clone());
                            final_response = location.unwrap();
                            info!("Local=>write {}", request);
                            continue;
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            error!("Local->Write error: {}", e);
                            continue;
                        }
                        Err(e) => {
                            error!("Local->Write error/v2: {}", e);
                            return Err(e.into());
                        }
                    }
                }
                Err(e) if max_attempts_to_write >= 0 => {
                    warn!("Local->Write error/v3: {}", e);
                    max_attempts_to_write -= 1;
                    let ten_millis = time::Duration::from_millis(25);
                    thread::sleep(ten_millis);
                    continue;
                }
                Err(e) => {
                    warn!("Local->Write error/v3: {}", e);
                    let result: String = String::from(final_response);
                    return Ok(result);
                }
            }
        }

        if (ready.is_read_closed() && ready.is_write_closed()) {
            info!("Local thread to be terminated");
            let result: String = String::from(final_response);
            return Ok(result);
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Error,
            Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(
            LevelFilter::Info,
            Config::default(),
            File::create("logs/tcpTokioServer.log").unwrap(),
        ),
    ])
    .unwrap();

    let local_listener = TcpListener::bind(LOCAL_RESOURCE).await?;

    let (socket, _) = local_listener.accept().await?;
    let res = process_local_stream(socket).await;
    match res {
        Ok(res_string) => {
            error!("Final result: {:?}", res_string);
            std::process::exit(0);
        }
        Err(e) => {
            error!("Final Error: {:?}", e);
            std::process::exit(-1);
        }
    }
}
