fn main() {
    println!(
        "{}",
        serde_json::json!({"request":stitch_nexus::request("test-request","model",&stitch_core::GenerateRequest{prompt:"hello".into(),max_tokens:8,local_only:false,raw_prompt:false,split_axis:None,preferred_chunk:None,connector_count:None,working_memory_bytes:None}).unwrap(),"result":stitch_nexus::result("test-request","model","hello",2)})
    );
}
