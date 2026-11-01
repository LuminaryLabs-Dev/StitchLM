//! Narrow Rust bridge to the pinned NexusEngine inference contract.
//! It does not claim to run NexusEngine's JavaScript Kits or its build orchestrator.
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use stitch_core::GenerateRequest;
pub const ENGINE_COMMIT: &str = "7d2cb0398887d4d09569cb44d20b11febb58e826";
pub fn request(id: &str, model_id: &str, input: &GenerateRequest) -> Result<Value> {
    ensure!(
        !id.is_empty() && !model_id.is_empty() && input.max_tokens > 0,
        "invalid inference request"
    );
    Ok(
        json!({"schema":"nexusengine.inference.request/1","id":id,"modelId":model_id,"messages":[{"role":"user","content":input.prompt}],"input":null,"metadata":{},"capabilities":[],"outputSchema":null,"tokenBudget":input.max_tokens,"parameters":{},"traceId":id}),
    )
}
pub fn result(request_id: &str, model_id: &str, text: &str, output_tokens: usize) -> Value {
    json!({"schema":"nexusengine.inference.result/1","requestId":request_id,"status":"ok","output":text,"usage":{"output_tokens":output_tokens},"receiptRef":null,"error":null,"modelId":model_id,"providerId":"stitchlm-cpu","deploymentId":null,"attempts":[],"traceId":request_id,"reportedCost":null,"estimatedCost":null,"usageComplete":false})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_usage_is_not_fabricated() {
        let r = result("r", "m", "hello", 2);
        assert_eq!(r["usageComplete"], false);
        assert!(r["usage"].get("input_tokens").is_none());
    }
}
