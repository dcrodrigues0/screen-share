use leptos::html::Input;
use leptos::task::spawn_local;
use leptos::{ev::SubmitEvent, prelude::*};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use leptos::logging::log;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    async fn invoke_without_args(cmd: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;
}

#[derive(Serialize)]
struct SessionArgs<'a> {
    sessionId: &'a str,
}

#[derive(Serialize)]
struct JoinSessionArgs<'a> {
    sessionId: &'a str,
}


#[component]
pub fn App() -> impl IntoView {
    let (session_id, set_session_id) = signal(String::new());
    let join_id: NodeRef<Input> = NodeRef::new();

    let (status, set_status) = signal(String::new());
    let (sharing, set_sharing) = signal(false);    

    let start_sharing = move |_| {
        spawn_local(async move {
            let response = invoke_without_args("start_screen_share")
            .await;

            if let Some(id) = response.as_string() {
                log!("Started screen sharing session with ID: {}", id);
                set_session_id.set(id.clone());
                set_sharing.set(true);
                set_status.set(format!("Your session ID is: {}", id));
            } else {
                set_status.set("Could not start screen sharing.".to_string());
            }
        });
    };

    let join_session = move |ev: SubmitEvent| {
        ev.prevent_default();
        
        log::info!("Join session button clicked");
        let input = join_id
            .get()
            .expect("Join input was not mounted");

        let id = input.value();

        if id.trim().is_empty() {
            set_status.set("Please enter a session ID.".to_string());
            return;
        }

        spawn_local(async move {
            let args = serde_wasm_bindgen::to_value(
                &JoinSessionArgs {
                    sessionId: &id,
                }
            )
            .unwrap();

            log!("Attempting to join session with ID: {}", id);
            let response = invoke("join_screen_share", args).await;

            if let Some(message) = response.as_string() {
                set_status.set(message);
            } else {
                set_status.set("Could not join the session.".to_string());
            }
        });
    };

    view! {
        <main class="app">

            <div class="header">
                <h1>"Screen Share"</h1>
                <p class="subtitle">
                    "Share your screen or join someone else's session."
                </p>
            </div>

            <section class="share-card">

                <div class="share-icon">
                    "👻"
                </div>

                <h2>
                    {move || {
                        if sharing.get() {
                            "You are sharing your screen"
                        } else {
                            "Share your screen"
                        }
                    }}
                </h2>

                <p class="description">
                    "Start a screen sharing session and send the generated ID to another person."
                </p>

                <button
                    class="share-button"
                    on:click=start_sharing
                    disabled=move || sharing.get()
                >
                    {move || {
                        if sharing.get() {
                            "Screen Sharing Active"
                        } else {
                            "Share My Screen"
                        }
                    }}
                </button>

                {move || {
                    if !session_id.get().is_empty() {
                        Some(view! {
                            <div class="session-box">
                                <span class="session-label">
                                    "Your session ID"
                                </span>

                                <strong>
                                    {move || session_id.get()}
                                </strong>

                                <p>
                                    "Send this ID to the person who wants to watch your screen."
                                </p>
                            </div>
                        })
                    } else {
                        None
                    }
                }}

            </section>

            <div class="divider">
                <span>"OR"</span>
            </div>

            <section class="join-card">
                <h2>"Join a screen share"</h2>
                <p class="description">
                    "Enter the session ID provided by the person sharing their screen."
                </p>
                <form on:submit=join_session>
                    <input
                        node_ref=join_id
                        type="text"
                        placeholder="Enter session ID"
                        autocomplete="off"
                    />

                    <button type="submit" class="join-button">
                        "Join Screen"
                    </button>
                </form>
            </section>

            {move || {
                if !status.get().is_empty() {
                    Some(view! {
                        <div class="status">
                            {move || status.get()}
                        </div>
                    })
                } else {
                    None
                }
            }}

        </main>
    }
}