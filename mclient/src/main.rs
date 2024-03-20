use anyhow::Result;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_derive::{Deserialize, Serialize};
use serde_json::Value;

mod varint_type;
use varint_type::*;

const CONTINUE_BIT: i32 = 0x80; // for packets in loop to allow writing in the command line

use std::net::TcpStream;

#[derive(Debug, Serialize, Deserialize)]
struct ChatComponent {
    insertion: Option<String>,
    text: Option<String>,
    color: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    underlined: Option<bool>,
    strikethrough: Option<bool>,
    obfuscated: Option<bool>,
    font: Option<String>,
}

fn get_chat_message(json_string: &str) {
    let mut formatted_text = String::new();

    if let Ok(json) = serde_json::from_str::<Value>(json_string) {
        if let Some(bold) = json.get("bold").and_then(|b| b.as_bool()) {
            if bold {
                formatted_text.push_str("\x1B[1m");
            }
        }
        if let Some(italic) = json.get("italic").and_then(|i| i.as_bool()) {
            if italic {
                formatted_text.push_str("\x1B[3m");
            }
        }

        if let Some(underlined) = json.get("underlined").and_then(|u| u.as_bool()) {
            if underlined {
                formatted_text.push_str("\x1B[4m");
            }
        }

        if let Some(strikethrough) = json.get("strikethrough").and_then(|s| s.as_bool()) {
            if strikethrough {
                formatted_text.push_str("\x1B[9m");
            }
        }

        if let Some(color) = json.get("color").and_then(|c| c.as_str()) {
            match color {
                "black" => formatted_text.push_str("\x1B[30m"),
                "dark_blue" => formatted_text.push_str("\x1B[34m"),
                "dark_green" => formatted_text.push_str("\x1B[32m"),
                "dark_aqua" => formatted_text.push_str("\x1B[36m"),
                "dark_red" => formatted_text.push_str("\x1B[31m"),
                "dark_purple" => formatted_text.push_str("\x1B[35m"),
                "gold" => formatted_text.push_str("\x1B[33m"),
                "gray" => formatted_text.push_str("\x1B[37m"),
                "dark_gray" => formatted_text.push_str("\x1B[90m"),
                "blue" => formatted_text.push_str("\x1B[94m"),
                "green" => formatted_text.push_str("\x1B[92m"),
                "aqua" => formatted_text.push_str("\x1B[96m"),
                "red" => formatted_text.push_str("\x1B[91m"),
                "light_purple" => formatted_text.push_str("\x1B[95m"),
                "yellow" => formatted_text.push_str("\x1B[93m"),
                "white" => formatted_text.push_str("\x1B[97m"),
                _ => {}
            }
        }

        let translate = json
            .get("translate")
            .and_then(|t| t.as_str())
            .unwrap_or_default();

        match translate {
            "chat.type.text" | "commands.message.display.incoming" => {
                if let Some(with) = json.get("with").and_then(|w| w.as_array()) {
                    if with.len() == 2 {
                        if let Some(insertion) = with[0].get("insertion").and_then(|i| i.as_str()) {
                            formatted_text.push_str(&format!("<{}> ", insertion));
                        }

                        if let Some(text) = with[1].as_str() {
                            formatted_text.push_str(text);
                        } else if let Some(text_obj) = with[1].get("text").and_then(|t| t.as_str())
                        {
                            formatted_text.push_str(text_obj);
                        }
                    }
                }
            }
            "multiplayer.player.left" => {
                if let Some(with) = json.get("with").and_then(|w| w.as_array()) {
                    if with.len() == 1 {
                        if let Some(text) = with[0].as_str() {
                            formatted_text.push_str(&format!("{} left the game", text));
                        } else if let Some(text_obj) = with[0].get("text").and_then(|t| t.as_str())
                        {
                            formatted_text.push_str(&format!("{} left the game", text_obj));
                        }
                    }
                }
            }
            "multiplayer.player.joined" => {
                if let Some(with) = json.get("with").and_then(|w| w.as_array()) {
                    if with.len() == 1 {
                        if let Some(text) = with[0].as_str() {
                            formatted_text.push_str(&format!("{} joined the game", text));
                        } else if let Some(text_obj) = with[0].get("text").and_then(|t| t.as_str())
                        {
                            formatted_text.push_str(&format!("{} joined the game", text_obj));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    println!("{}\x1B[0m", formatted_text);
}

fn connect_to_server() -> Result<TcpStream, io::Error> {
    let stream: TcpStream = TcpStream::connect("127.0.0.1:25565")?;
    Ok(stream)
}

fn console_reader(shared_command_queue: Arc<Mutex<Vec<String>>>) {
    loop {
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                let trimmed = input.trim().to_string();
                if !trimmed.is_empty() {
                    let mut command_queue = shared_command_queue.lock().unwrap();
                    command_queue.push(trimmed);
                }
            }
            Err(error) => eprintln!("Error reading from console: {:?}", error),
        }
    }
}

fn handshake(stream: &mut TcpStream, state: i32) -> io::Result<()> {
    let mut send_handshake: Vec<u8> = vec![];
    send_handshake.append(&mut varint_write(0i32)); // id
    let mut protocol_version: Vec<u8> = varint_write(758); // protocol version
    send_handshake.append(&mut protocol_version);
    let address: String = "127.0.0.1".to_string();
    send_handshake.append(&mut varint_write(address.len() as i32)); //addres length
    send_handshake.append(&mut address.as_bytes().to_vec());
    let port: u16 = 25565;
    send_handshake.append(&mut port.to_be_bytes().to_vec());
    let mut next_state: Vec<u8> = varint_write(state); // 1 for status request,
    send_handshake.append(&mut next_state);

    let mut packet_length: Vec<u8> = varint_write(send_handshake.len() as i32);

    packet_length.append(&mut send_handshake);

    stream.write_all(&packet_length)?;

    Ok(())
}

fn print_status_and_save_favicon(buf: &mut Vec<u8>) -> io::Result<()> {
    let json_length = varint_read(buf).unwrap() as usize; // json length

    let json_data = String::from_utf8(buf.drain(..json_length).collect()).unwrap();

    println!("Server status: {}", json_data);

    // Writing JSON data to file
    let mut f = File::create("status_response.json")?;
    f.write_all(json_data.as_bytes()).unwrap();

    // saving the favicon

    let pattern = "\"favicon\":\"data:image/png;base64,".to_string();

    let pattern_length = pattern.len();

    let start_index = json_data
        .find("\"favicon\":\"data:image/png;base64,")
        .unwrap()
        + pattern_length;

    let remaining_data = &json_data[start_index..];

    let end_index = remaining_data.find('\"').unwrap();

    let favicon_data = &remaining_data[..end_index];

    let decoded_data = STANDARD.decode(favicon_data).unwrap();

    let mut png_file = File::create("server-icon.png")?;
    png_file.write_all(&decoded_data)?;

    println!("\x1B[95mServer icon saved!\x1B[0m");

    Ok(())
}

fn help_command() {
    println!("Commands:");
    println!("list: shows the online players");
    println!("status: prints the server status and downloads the server icon");
    println!("help: shows the commands");
    println!("quit: disconnects from the server");
    println!("any other commands: sends a chat message to the server with the string");
}

fn request_status(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    // Send the Status Request packet (Packet ID: 0x00)

    let mut send_request_status: Vec<u8> = vec![];

    send_request_status.append(&mut varint_write(0i32)); // packet id

    let mut packet_length: Vec<u8> = varint_write(send_request_status.len() as i32); // packet length

    packet_length.append(&mut send_request_status); // prefixed by packet length

    stream.write_all(&packet_length)?; // sending to player

    // --------reading status response--------
    // format:
    //   length
    //   id
    //   length of json
    //   json
    let buffer_len: usize = 32767;
    let mut buf = vec![0u8; 32767];
    let read_bytes = stream.read(&mut buf).unwrap();

    if read_bytes <= buffer_len {
        // Extracting length, id, and json length
        let _length = varint_read(&mut buf).unwrap();

        let _id = varint_read(&mut buf).unwrap();

        Ok(buf)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Error reading from stream",
        ))
    }
}

fn login_start(stream: &mut TcpStream) -> io::Result<()> {
    // println!("Sending Login Start packet...");

    let mut login_start_packet: Vec<u8> = vec![];

    login_start_packet.append(&mut varint_write(0i32)); //id 0x00

    let username: String = "eudinaltapartee".to_string(); // username

    login_start_packet.append(&mut varint_write(username.len() as i32)); // username length

    login_start_packet.append(&mut username.as_bytes().to_vec()); // sending player username

    let mut packet_length: Vec<u8> = varint_write(login_start_packet.len() as i32);

    packet_length.append(&mut login_start_packet);

    stream.write_all(&packet_length).unwrap();

    // println!("Login Start packet sent.");

    Ok(())
}

fn login_succes(stream: &mut TcpStream) -> io::Result<()> {
    let buf_len: usize = 700;
    let mut buf = vec![0u8; 700];
    let bytes_read = stream.read(&mut buf).unwrap();

    if bytes_read <= buf_len {
        // length
        // id
        // uuid
        // user length
        // number of properties?
        // property?

        varint_read(&mut buf).unwrap(); // reading packet length

        varint_read(&mut buf).unwrap(); // id
        let uuid: u128 =
            u128::from_be_bytes(buf.drain(..16).collect::<Vec<u8>>().try_into().unwrap());
        let username_length = varint_read(&mut buf).unwrap() as usize;

        let username =
            String::from_utf8(buf.drain(..username_length).collect::<Vec<u8>>()).unwrap();

        println!(
            "User connected with username: {} and uuid: {}",
            username, uuid
        );

        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Error reading from stream",
        ))
    }
}

fn ping_request(stream: &mut TcpStream) -> io::Result<()> {
    //println!("Sending Ping Request packet...");

    let mut ping_request_packet: Vec<u8> = vec![];

    ping_request_packet.append(&mut varint_write(1i32)); //id 0x01

    let payload: u64 = 92233720u64;

    ping_request_packet.append(&mut payload.to_be_bytes().to_vec());

    let mut packet_length: Vec<u8> = varint_write(ping_request_packet.len() as i32);

    packet_length.append(&mut ping_request_packet);

    stream.write_all(&packet_length).unwrap();

    Ok(())
}

fn ping_response(stream: &mut TcpStream) -> io::Result<()> {
    let buf_len: usize = 700;
    let mut buf = vec![0u8; 700];
    let bytes_read = stream.read(&mut buf).unwrap();

    if bytes_read <= buf_len {
        // length
        // id
        // payload

        varint_read(&mut buf).unwrap();

        varint_read(&mut buf).unwrap();

        let payload_from_ping_request: u64 = 92233720u64;
        let payload_received: u64 =
            u64::from_be_bytes(buf.drain(..8).collect::<Vec<u8>>().try_into().unwrap());

        if payload_from_ping_request == payload_received {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "Error at ping - pong"))
        }
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Error reading from stream",
        ))
    }
}

fn keep_alive(stream: &mut TcpStream, mut payload: Vec<u8>) -> io::Result<()> {
    let mut keep_alive_packet: Vec<u8> = vec![];

    keep_alive_packet.append(&mut varint_write(0x0Fi32)); //id 0x21

    keep_alive_packet.append(&mut payload);

    let mut packet_length: Vec<u8> = varint_write(keep_alive_packet.len() as i32);

    packet_length.append(&mut keep_alive_packet);

    stream.write_all(&packet_length).unwrap();

    Ok(())
}

fn pong(stream: &mut TcpStream, mut payload: Vec<u8>) -> io::Result<()> {
    println!("Sending Pong...");

    let mut pong_packet: Vec<u8> = vec![];

    pong_packet.append(&mut varint_write(0x30i32)); //id 0x21

    pong_packet.append(&mut payload);

    let mut packet_length: Vec<u8> = varint_write(pong_packet.len() as i32);

    packet_length.append(&mut pong_packet);

    stream.write_all(&packet_length).unwrap();

    println!("Pong packet sent.");

    Ok(())
}

fn update_player_list(
    buf: &mut Vec<u8>,
    number_of_players: i32,
    online_players: &mut HashMap<u128, String>,
) -> io::Result<()> {
    // println!("Updating Player List...");
    // packet format
    // uuid u128
    // name_length (varint)
    // name (string)
    // number of properties (varint)
    // array of properties
    // property_name_length (varint)
    // property_name (string)
    // property_value_length (varint)
    // property_value (string)
    // property_is_signed (bool) - daca e true, mai avem 2 campuri
    // property_signature_length (varint)
    // property_signature (string)
    // game_mode (varint)
    // ping (varint)
    // has_display_name (bool)
    // display_name (nbt)

    for _player in 0..number_of_players {
        let uuid: u128 =
            u128::from_be_bytes(buf.drain(..16).collect::<Vec<u8>>().try_into().unwrap());

        let username_length = varint_read(buf).unwrap() as usize;

        let username =
            String::from_utf8(buf.drain(..username_length).collect::<Vec<u8>>()).unwrap();
        online_players.entry(uuid).or_insert(username);

        let number_of_properties = varint_read(buf).unwrap();

        for _ in 0..number_of_properties {
            let property_name_length = varint_read(buf).unwrap() as usize; // property name
            String::from_utf8(buf.drain(..property_name_length).collect::<Vec<u8>>()).unwrap();

            let property_value_length = varint_read(buf).unwrap() as usize; // property value
            String::from_utf8(buf.drain(..property_value_length).collect::<Vec<u8>>()).unwrap();

            let property_is_signed = varint_read(buf).unwrap();

            if property_is_signed == 1 {
                let property_signature_length = varint_read(buf).unwrap() as usize; // property signature
                String::from_utf8(buf.drain(..property_signature_length).collect::<Vec<u8>>())
                    .unwrap();
            }

            varint_read(buf).unwrap(); // game mode

            varint_read(buf).unwrap(); // ping

            let has_display_name = varint_read(buf).unwrap();

            if has_display_name == 1 {
                let display_name_length = varint_read(buf); // display name length
                String::from_utf8(
                    buf.drain(..display_name_length.unwrap() as usize)
                        .collect::<Vec<u8>>(),
                )
                .unwrap();
            }
        }
    }
    Ok(())
}

fn remove_from_online_players(
    buf: &mut Vec<u8>,
    number_of_players: i32,
    online_players: &mut HashMap<u128, String>,
) -> io::Result<()> {
    for _player in 0..number_of_players {
        let uuid: u128 =
            u128::from_be_bytes(buf.drain(..16).collect::<Vec<u8>>().try_into().unwrap());

        online_players.remove(&uuid);
    }

    Ok(())
}

fn player_info(mut buf: Vec<u8>, online_players: &mut HashMap<u128, String>) -> io::Result<()> {
    let action: i32 = varint_read(&mut buf).unwrap(); // action

    let number_of_players = varint_read(&mut buf).unwrap(); // player number

    if action == 0 {
        update_player_list(&mut buf, number_of_players, online_players).unwrap();
    } else if action == 4 {
        remove_from_online_players(&mut buf, number_of_players, online_players).unwrap();
    }

    Ok(())
}

fn send_chat_message(stream: &mut TcpStream, message: &str) -> io::Result<()> {
    let mut chat_message_packet: Vec<u8> = vec![];

    chat_message_packet.append(&mut varint_write(0x03i32)); //id 0x03

    chat_message_packet.append(&mut varint_write(message.len() as i32));

    chat_message_packet.append(&mut message.as_bytes().to_vec());

    let mut packet_length: Vec<u8> = varint_write(chat_message_packet.len() as i32); // packet length

    packet_length.append(&mut chat_message_packet);

    stream.write_all(&packet_length).unwrap();

    Ok(())
}

fn receive_chat_message(mut buf: Vec<u8>) -> io::Result<()> {
    let chat_message_length = varint_read(&mut buf).unwrap() as usize;

    let chat_message =
        String::from_utf8(buf.drain(..chat_message_length).collect::<Vec<u8>>()).unwrap();

    let _position: u8 = buf.remove(0); // position

    let _sender_uuid: u128 =
        u128::from_be_bytes(buf.drain(..16).collect::<Vec<u8>>().try_into().unwrap()); // sender uuid

    get_chat_message(&chat_message);

    Ok(())
}

fn main() -> io::Result<()> {
    let shared_command_queue: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let command_queue_clone: Arc<Mutex<Vec<String>>> = shared_command_queue.clone();

    thread::spawn(move || {
        console_reader(command_queue_clone);
    });

    let mut response_status_for_printing: Vec<u8> = vec![];

    {
        if let Ok(mut status_stream) = connect_to_server() {
            // Perform a handshake

            handshake(&mut status_stream, 1).unwrap();
            response_status_for_printing = request_status(&mut status_stream).unwrap(); // saving for future status request commands
            ping_request(&mut status_stream).unwrap();
            ping_response(&mut status_stream).unwrap();
        } else {
            println!("Failed to connect to the server.");
        }
    }

    if let Ok(mut stream) = connect_to_server() {
        handshake(&mut stream, 2).unwrap();
        login_start(&mut stream).unwrap();
        login_succes(&mut stream).unwrap();

        let mut online_players: HashMap<u128, String> = HashMap::new();

        loop {
            // reading packets

            let mut command_queue = shared_command_queue.lock().unwrap();
            for command in command_queue.iter() {
                match command.as_str() {
                    "list" => {
                        println!("Online Players: {:?}", online_players);
                    }
                    "help" => {
                        help_command();
                    }
                    "status" => {
                        print_status_and_save_favicon(&mut response_status_for_printing).unwrap();
                    }
                    "quit" => {
                        println!("Ok, quitting");
                        return Ok(());
                    }
                    _ => {
                        send_chat_message(&mut stream, command).unwrap();
                    }
                }
            }
            command_queue.clear();

            let mut packet_varint: Vec<u8> = vec![];

            loop {
                let mut init = vec![0u8];
                stream.read_exact(&mut init).unwrap();
                let current_byte = init.remove(0);
                if (current_byte as i32 & CONTINUE_BIT) == 0 {
                    packet_varint.push(current_byte);
                    break;
                } else {
                    packet_varint.push(current_byte);
                }
            }

            let packet_length = varint_read(&mut packet_varint).unwrap() as usize;

            let mut buf_packet: Vec<u8> = vec![0u8; packet_length];
            stream.read_exact(&mut buf_packet).unwrap(); // the packet is in buf_packet starting with id

            let id = varint_read(&mut buf_packet).unwrap();

            if id == varint_read(vec![0x21].as_mut()).unwrap() {
                keep_alive(&mut stream, buf_packet).unwrap();
            } else if id == varint_read(vec![0x30].as_mut()).unwrap() {
                pong(&mut stream, buf_packet).unwrap();
            } else if id == varint_read(vec![0x36].as_mut()).unwrap() {
                // player info
                player_info(buf_packet, &mut online_players).unwrap();
            } else if id == varint_read(vec![0x3C].as_mut()).unwrap() {
                // player info update
                println!("received player info update"); // nu l trimite aparent
            } else if id == varint_read(vec![0x0F].as_mut()).unwrap() {
                // chat message clientbound
                receive_chat_message(buf_packet).unwrap();
            } else if id == varint_read(vec![0x1A].as_mut()).unwrap() {
                // client disconnected
                println!("Player disconnected");
                return Ok(());
            }
        }
    } else {
        println!("Failed to connect to the server.");
    }

    Ok(())
}
