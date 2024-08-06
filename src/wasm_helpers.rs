use wasm_bindgen::prelude::*;
#[wasm_bindgen(
inline_js = "
    function wait(ms) {
       return new Promise((_, reject) => {
          setTimeout(() => reject(new Error('timeout succeeded')), ms);
       });
    }
    export fn js_timeout(ms, promise) {
        await Promise.race([wait(ms), promise()])
    }
    ")
]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn js_timeout(promise: js_sys::Promise) -> Result<JsValue, JsValue>;
}

use std::time::Duration;
use std::future::{Future, IntoFuture};

/*
pub(crate) async fn timeout<F>(duration: Duration, future: F) -> Result<F::Output, JsValue>
where
    F: Future,
{
    let promise = wasm_bindgen_futures::future_to_promise(future);
    let out = js_timeout(promise).await;
    todo!()
}
*/
