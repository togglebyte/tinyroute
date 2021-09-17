# TODO

## RouterTx

* [ ] Any failure to send to the router is unrecoverable 
      and should result in shutdown.
* [ ] Oneshot on agent creation


## Transports

* [X] Add `Local` and `Remote` transport
* [X] Router should send an internal message (not `AgentMsg`) to the agent which 
      contains information if this is a local or remote message.
* [X] Agent should expose a message type with three variants:
    * [X] `Message`
    * [X] `ShutDown`
    * [X] `AgentRemoved`


## Connection layout

`Reader(impl AsyncRead, router_tx, Path)` 
    -> Router 
        -> `Connection(impl WriteAsync)`

* [.] Connection
    * [ ] Input: Send data from socket to the `Connection` via router (path)
    * [X] Output: Data from `Connection` written to the socket
    * [ ] Shut down 
        * [ ] `Connection` on socket closure
        * [ ] reader on `Connection` closure


```rust
{ // Writer block, not even close to the reader block
    // Writer 
    
    async fn run(mut connection) {
        let msg = connection.recv().await;
        match msg {
            AgentMsg::Shutdown => break, 
            AgentMsg::Message(payload) => {
                let data = msg.serialize_and_frame();
                self.writer.write(data).await;
            }
            _ => {}
        }
    
        drop(connetion); // this is not really required
    }
}

{ // Read from the socket
    let socket = {};
    let conn_tx = {}; // how we send data to the `Connection`
    while let Some(msg) = socket.read() {
        conn_tx.send(AgentMsg::Message(msg));
    }
    let _ = conn_tx.send(AgentMsg::ShutDown); // might fail
}
```



* [O] Agent to Agent control
    * [ ] Agent A can shut down Agent B
    * [X] Agent A is notified when Agent B is removed
    * [X] Agent A --> Agent B
    * [X] Agent A <-- Agent B
    * [X] Clean up when removing an agent
* [ ] Better use of warn / info / error
* [ ] Move all the networking out of the agent / router code into its own lib
* [ ] Connection closed:
    * [ ] Unregister from any services (not services any more)
    
    

## Chat example:

Room manager:
* Create new rooms
* Destroy room
* Spawn a task with a `Room`

Room:
* Handle members <Vec<String, mpsc::Sender<Message>>>
* Pass messages { sender: String, message: String }
