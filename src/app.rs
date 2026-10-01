use leptos::task::spawn_local;
use leptos::{ev::SubmitEvent, prelude::*};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;


#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;
}

#[derive(Serialize)]
struct SessionArgs<'a> {
    session_id: &'a str,
}

#[derive(Serialize)]
struct JoinSessionArgs<'a> {
    session_id: &'a str,
}


#[component]
pub fn App() -> impl IntoView {
    let (session_id, set_session_id) = signal(String::new());
    let (join_id, set_join_id) = signal(String::new());
    let (status, set_status) = signal(String::new());
    let (sharing, set_sharing) = signal(false);    

    // Start sharing the screen
    let start_sharing = move |_| {
        log::info!("Starting screen sharing session...");
        spawn_local(async move {
            let response = invoke(
                "start_screen_share",
                JsValue::NULL,
            )
            .await;

            if let Some(id) = response.as_string() {
                set_session_id.set(id.clone());
                set_sharing.set(true);
                set_status.set(format!("Your session ID is: {}", id));
            } else {
                set_status.set("Could not start screen sharing.".to_string());
            }
        });
    };

    // Join an existing screen share
    let join_session = move |ev: SubmitEvent| {
        ev.prevent_default();
        let id = join_id.get_untracked();
        log::info!("Attempting to join session with ID: {}", id);
        if id.trim().is_empty() {
            set_status.set("Enter a session ID.".to_string());
            return;
        }

        spawn_local(async move {
            let args = serde_wasm_bindgen::to_value(
                &JoinSessionArgs {
                    session_id: &id,
                }
            )
            .unwrap();

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
                        type="text"
                        placeholder="Enter session ID"
                        autocomplete="off"
                        prop:value=move || join_id.get()
                        on:input=move |ev| {
                            set_join_id.set(event_target_value(&ev));
                        }
                    />

                    <button
                        type="submit"
                        class="join-button"
                    >
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