//! Platform adapters stay outside the inference worker.
pub fn valid_endpoint(url: &str) -> bool {
    url.starts_with('/') && !url.starts_with("//") && !url.contains('\\') && !url.contains('#')
}
#[cfg(target_arch = "wasm32")]
pub mod web {
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    async fn request_inner(
        path: &str,
        token: &str,
        method: &str,
        body: Option<&str>,
        allow_conflict: bool,
    ) -> Result<web_sys::Response, JsValue> {
        if !super::valid_endpoint(path) {
            return Err(JsValue::from_str("same-origin relative API path required"));
        }
        let opts = web_sys::RequestInit::new();
        opts.set_method(method);
        opts.set_mode(web_sys::RequestMode::SameOrigin);
        if let Some(body) = body {
            opts.set_body(&JsValue::from_str(body));
        }
        let request = web_sys::Request::new_with_str_and_init(path, &opts)?;
        request
            .headers()
            .set("Authorization", &format!("Bearer {token}"))?;
        if body.is_some() {
            request.headers().set("Content-Type", "application/json")?;
        }
        let global = js_sys::global();
        let fetch = js_sys::Reflect::get(&global, &JsValue::from_str("fetch"))?
            .dyn_into::<js_sys::Function>()?;
        let promise = fetch
            .call1(&global, &request)?
            .dyn_into::<js_sys::Promise>()?;
        let response = JsFuture::from(promise)
            .await?
            .dyn_into::<web_sys::Response>()?;
        if !response.ok() && !(allow_conflict && response.status() == 409) {
            let status = response.status();
            let message = JsFuture::from(response.text()?)
                .await?
                .as_string()
                .unwrap_or_default();
            return Err(JsValue::from_str(&format!("HTTP {status}: {message}")));
        }
        Ok(response)
    }
    pub async fn request(
        path: &str,
        token: &str,
        method: &str,
        body: Option<&str>,
    ) -> Result<web_sys::Response, JsValue> {
        request_inner(path, token, method, body, false).await
    }
    pub async fn request_allow_conflict(
        path: &str,
        token: &str,
        method: &str,
        body: Option<&str>,
    ) -> Result<web_sys::Response, JsValue> {
        request_inner(path, token, method, body, true).await
    }
    pub async fn text(
        path: &str,
        token: &str,
        method: &str,
        body: Option<&str>,
    ) -> Result<String, JsValue> {
        let r = request(path, token, method, body).await?;
        Ok(JsFuture::from(r.text()?)
            .await?
            .as_string()
            .unwrap_or_default())
    }
    pub async fn pause(ms: i32) -> Result<(), JsValue> {
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            let global = js_sys::global();
            let result = (|| {
                let timer = js_sys::Reflect::get(&global, &"setTimeout".into())?
                    .dyn_into::<js_sys::Function>()?;
                timer.call2(&global, &resolve, &ms.into())
            })();
            if let Err(e) = result {
                let _ = reject.call1(&JsValue::NULL, &e);
            }
        });
        JsFuture::from(promise).await?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn path_is_same_origin() {
        assert!(super::valid_endpoint("/api/status"));
        for p in ["https://evil/", "//evil/", "/\\evil/"] {
            assert!(!super::valid_endpoint(p));
        }
    }
}
