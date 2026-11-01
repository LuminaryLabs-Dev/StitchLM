//! Shared browser UI primitives. Runtime and secrets are never stored in the DOM.
use wasm_bindgen::{JsCast, JsValue};
pub fn document() -> Result<web_sys::Document, JsValue> {
    web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("document unavailable"))
}
pub fn element(id: &str) -> Result<web_sys::Element, JsValue> {
    document()?
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("missing element {id}")))
}
pub fn value(id: &str) -> Result<String, JsValue> {
    let e = element(id)?;
    if let Some(i) = e.dyn_ref::<web_sys::HtmlSelectElement>() {
        return Ok(i.value());
    }
    if let Some(i) = e.dyn_ref::<web_sys::HtmlInputElement>() {
        return Ok(i.value());
    }
    if let Some(i) = e.dyn_ref::<web_sys::HtmlTextAreaElement>() {
        return Ok(i.value());
    }
    Err(JsValue::from_str("not an input"))
}
pub fn text(id: &str, value: &str) {
    if let Ok(e) = element(id) {
        e.set_text_content(Some(value));
    }
}
pub fn error(e: JsValue) {
    text(
        "status",
        &format!(
            "Error: {}",
            e.as_string().unwrap_or_else(|| format!("{e:?}"))
        ),
    );
}
pub fn click(id: &str, f: impl FnMut(web_sys::Event) + 'static) -> Result<(), JsValue> {
    let handler =
        wasm_bindgen::closure::Closure::wrap(Box::new(f) as Box<dyn FnMut(web_sys::Event)>);
    element(id)?.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref())?;
    handler.forget();
    Ok(())
}
