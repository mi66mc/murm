#![allow(dead_code)]
use murm_protocol::*;
use serde_json::{json,Value};
use std::{sync::{Arc,Mutex},collections::VecDeque,time::Duration};
pub fn event(name:&str,time:u64)->VerifiedEvent{
    sign_event(&UnsignedEvent{version:1,signer:derive_public_key(&[1;32]),kind:1,created_at:time,
        header:json!({"publication":name,"revision":1,"status":"published"}).as_object().unwrap().clone(),content:json!(name)},&[1;32]).unwrap()
}
pub fn batch(events:Vec<Event>,cursor:&str)->String{json!({"events":events,"cursor":cursor,"has_more":false}).to_string()}
#[derive(Default)]
pub struct Script {pub responses:VecDeque<(u16,String)>,pub requests:Vec<(String,Value)>,pub delay:Duration}
pub struct Server {pub url:String,pub script:Arc<Mutex<Script>>,task:tokio::task::JoinHandle<()>}
impl Drop for Server{fn drop(&mut self){self.task.abort();}}
pub async fn server(responses:Vec<(u16,String)>)->Server{
    let script=Arc::new(Mutex::new(Script{responses:responses.into(),..Default::default()}));
    async fn reply(axum::extract::State(state):axum::extract::State<Arc<Mutex<Script>>>,method:axum::http::Method,body:axum::body::Bytes)->axum::response::Response{
        let (delay,response)={
            let mut script=state.lock().unwrap();script.requests.push((method.to_string(),serde_json::from_slice(&body).unwrap_or(Value::Null)));
            (script.delay,script.responses.pop_front().unwrap_or((500,String::new())))
        };
        tokio::time::sleep(delay).await;
        let mut response=axum::response::Response::builder().status(response.0).header("content-type","application/json").body(axum::body::Body::from(response.1)).unwrap();
        if response.status()==429{response.headers_mut().insert("retry-after",axum::http::HeaderValue::from_static("60"));}
        response
    }
    let router=axum::Router::new().route("/info",axum::routing::get(||async{axum::Json(RelayInfo::default())}))
        .route("/events/sync",axum::routing::any(reply)).route("/events/query",axum::routing::any(reply))
        .route("/events",axum::routing::any(reply)).with_state(script.clone());
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let url=format!("http://{}",listener.local_addr().unwrap());
    let task=tokio::spawn(async move{axum::serve(listener,router).await.unwrap();});
    Server{url,script,task}
}
