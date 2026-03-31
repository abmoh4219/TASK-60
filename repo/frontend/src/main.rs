//! RailOps frontend — minimal Yew skeleton.
//! Domain components and pages are added in subsequent steps.

use yew::prelude::*;

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<App>::new().render();
}

#[function_component(App)]
fn app() -> Html {
    html! {
        <div>
            <h1>{ "RailOps" }</h1>
            <p>{ "System initialising…" }</p>
        </div>
    }
}
