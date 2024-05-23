use std::{borrow::Borrow, collections::HashMap};


const CHANNELS: &[&str] = &["#krapmatt"];
#[derive(Debug)]
struct Queue {
    twitchname: String,
    bungiename: String,
}

impl Default for Queue {
    fn default() -> Self {
        Queue { twitchname: "Empty".to_string(), bungiename: "Empty".to_string() }
    }
}
impl PartialEq for Queue {
    fn eq(&self, other: &Self) -> bool {
        if self.twitchname == other.twitchname {
            return true
        } else {
            return false
        }
    }
}



#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let oauth_token = "gbmw5v39axrv7q2xdzyhneph5hi7vi"; 
    let nickname = "krapbott";   
    
    let mut queue: Vec<Queue> = vec![];

    let credentials = tmi::Credentials::new(nickname, oauth_token);
    let mut client = tmi::Client::builder().credentials(credentials).connect().await.unwrap();
    
    let mut bungiename: &str;

    // Join the specified channel
    client.join_all(CHANNELS).await?;
    loop {
        let msg = client.recv().await?;
        match msg.as_typed()? {
            tmi::Message::Privmsg(msg) => {
                println!("{}: {}", msg.sender().name(), msg.text());
                if msg.text().contains("!join") {
                    let message = msg.text().split_once(" ");
                        if message.is_some() {
                            (_, bungiename) = message.unwrap();
                            if bungiename.contains("#") {
                                queue.push(Queue { twitchname: msg.sender().name().to_string(), bungiename: bungiename.to_string() });
                                
                                let printmsg = format!("{} entered the queue on position #{}", msg.sender().name(), queue.len());
                                client.privmsg("#krapmatt", &printmsg).send().await?;
                                println!("{:?}", queue);
                            } else {
                                let printmsg = format!("You have entered with wrong bungiename {}", msg.sender().name());
                                client.privmsg("#krapmatt", &printmsg).send().await?;
                            }    
                        }
 
                } if msg.text().contains("!next") {
                    let mut i = 0;
                    while i < 5 {
                        if !queue.is_empty() {
                            queue.remove(0);
                        }
                        
                        i += 1;
                    }
                    let queue_msg = format!("Next: {:?}, {:?}, {:?}, {:?}, {:?}", queue.get(0), queue.get(1), queue.get(2), queue.get(3), queue.get(4));
                    client.privmsg("#krapmatt", &queue_msg).send().await?;

                } if msg.text().contains("!remove") {
                    //remove one player from queue TODO!
                    let twitchname;
                    
                    let message = msg.text().split_once(" ");

                        if message.is_some() {
                            (_, twitchname) = message.unwrap();
                            
                            if queue.contains(&Queue { twitchname: twitchname.to_string().clone(), bungiename: "Any".to_string()}) {
                                
                                if let Some(index) = queue.iter().position(|x| x == &Queue{twitchname: twitchname.to_string(), bungiename: "any".to_string()}) {
                                    queue.remove(index);
                                    println!("{:?}", queue);
                                    let leave_message = format!("{} has been removed from queue", twitchname);
                                    client.privmsg("#krapmatt", &leave_message).send().await?;
                                }
                            }
                        }
                } if msg.text().contains("!pos") {
                    //shows position in queue
                    let user_name = msg.sender().name().to_string();
                    println!("{}", user_name);
                    let (index, group) = position(user_name, &queue);
                    let pos_msg = format!("You are on the position {} and in group {}", index, group);
                    client.privmsg("#krapmatt", &pos_msg).send().await?;
                } if msg.text().contains("!leave") {
                    //player wants to leave queue
                    let twitchname = msg.sender().name().to_string();
      
                    if queue.contains(&Queue { twitchname: twitchname.to_string().clone(), bungiename: "Any".to_string()}) {
                        if let Some(index) = queue.iter().position(|x| x == &Queue{twitchname: twitchname.to_string(), bungiename: "any".to_string()}) {
                            queue.remove(index);
                            println!("{:?}", queue);
                            let leave_message = format!("You have been removed from queue, {}", twitchname);
                            client.privmsg("#krapmatt", &leave_message).send().await?;
                        }
                    }
                        
                } if msg.text().contains("!queue") {
                    let mut twitch_name:Vec<String> = vec![];
                    for queue in &queue {
                        twitch_name.push(queue.twitchname.clone()) 
                    }
                    
                    let queue_str = format!("Queue: {:?}", twitch_name);
                    client.privmsg("#krapmatt", &queue_str).send().await?;
                }
          }
          tmi::Message::Reconnect => {
            client.reconnect().await?;
            client.join_all(CHANNELS).await?;
          }
          tmi::Message::Ping(ping) => {
            client.pong(&ping).await?;
          }
          _ => {}
        }
      }
    
}


fn position(user_name: String, queue: &Vec<Queue>) -> (String, usize) {
    if queue.contains(&Queue { twitchname: user_name.clone(), bungiename: "Any".to_string()}) {
        if let Some(mut index) = queue.iter().position(|x| x == &Queue{twitchname: user_name.to_string(), bungiename: "any".to_string()}) {
            println!("{}", index);
            index += 1;
            return (index.to_string(), index/5);
        } else {
            return ("error".to_string(), 0)
        }
    } else {
        return ("Not in queue".to_string(), 0)
    }
}