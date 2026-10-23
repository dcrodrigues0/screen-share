use leptos::html::Input;
use leptos::html::Video;
use leptos::task::spawn_local;
use leptos::{ev::SubmitEvent, prelude::*};
use serde::{Deserialize, Serialize};
use leptos::logging::log;
use leptos_use::{
    use_clipboard, UseClipboardReturn,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{DisplayMediaStreamConstraints, HtmlVideoElement, MediaStreamConstraints};
use uuid::Uuid;

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
    
    let video_ref: NodeRef<Video> = NodeRef::new();

    let stop_share = move |_| {
        if let Some(video) = video_ref.get(){
            let ss: Option<web_sys::MediaStream> = video.src_object();
            match ss {
                Some(stream) => {
                    stream.get_tracks().for_each(&mut |track,_,_|{
                        if let Ok(track) = track.dyn_into::<web_sys::MediaStreamTrack>() {
                            track.stop();
                            video.set_src_object(None);
                            set_sharing.set(false);
                            set_status.set("Session closed".to_string());
                        }
                    });
                }
                None => {
                    set_status.set("Cannot close screen share".to_string());
                },
            }
        }
    };

    let start_share = move |_| {
        let window = match web_sys::window() {
            Some(window) => window,
            None => {
                set_status.set("Window API unavailable".to_string());
                return;
            }
        };

        let secure = window.is_secure_context();
        let navigator = window.navigator();

        let media_devices = match navigator.media_devices() {
            Ok(devices) => devices,
            Err(err) => {
                set_status.set(format!(
                    "MediaDevices unavailable: {:?}",
                    err
                ));
                return;
            }
        };

        log!("Secure context: {}", window.is_secure_context());

        let constraints = DisplayMediaStreamConstraints::new();
        constraints.set_video(&JsValue::TRUE);
        constraints.set_audio(&JsValue::TRUE);

        let promise = match media_devices
            .get_display_media_with_constraints(&constraints){
            Ok(promise) => promise,
            Err(err) => {
                set_status.set(format!(
                    "getDisplayMedia call failed: {:?}",
                    err
                ));
                return;
            }
        };

        spawn_local(async move {
            match JsFuture::from(promise).await {
                Ok(value) => {
                    let stream: web_sys::MediaStream = value.unchecked_into::<web_sys::MediaStream>();

                    if let Some(video) = video_ref.get() {
                        let video: &HtmlVideoElement = video.unchecked_ref();
                        video.set_src_object(Some(&stream));

                        if let Err(err) = video.play() {
                            log!("Video playback error: {:?}", err);
                        }

                        let id = Uuid::new_v4().to_string();
                        set_session_id.set(id.clone());
                        set_sharing.set(true);
                        set_status.set(format!("Your session ID is: {}", id));
                    }
                }
                Err(err) => {
                    let name = js_sys::Reflect::get(
                        &err,
                        &JsValue::from_str("name"),
                    )
                    .ok()
                    .and_then(|v| v.as_string())
                    .unwrap_or_else(|| "UnknownError".to_string());

                    let message = js_sys::Reflect::get(
                        &err,
                        &JsValue::from_str("message"),
                    )
                    .ok()
                    .and_then(|v| v.as_string())
                    .unwrap_or_else(|| format!("{:?}", err));

                    let details = format!("{}: {}", name, message);
                    log!("Screen capture failed: {}", details);
                    set_status.set(details);
                }
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

    let UseClipboardReturn { 
        is_supported, 
        text, 
        copied, 
        copy 
    } = use_clipboard();

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
                    on:click=start_share
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
                

                <video 
                    node_ref=video_ref
                    autoplay=true 
                    playsinline=true 
                    muted=false 
                    width="340" 
                    height="220"
                    class="video-preview"
                >
                </video> 
                

                {move || {
                    if sharing.get() {
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

                            <div>
                                <button 
                                    class="active-session-button"
                                    on:click=stop_share>
                                    "Stop Sharing"
                                </button>
                                <button 
                                    class="active-session-button" 
                                    on:click={
                                        let copy = copy.clone();
                                        move |_| {
                                            copy(&session_id.get());
                                        }}>
                                    "Copy Session ID"
                                </button>
                                {move || {
                                    if copied.get() {
                                        Some(view! {
                                            <span class="copy-confirmation">
                                                "Session ID Copied"
                                            </span>
                                        })
                                    } else {
                                        None
                                    }
                                }}
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