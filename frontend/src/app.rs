use leptos::html::Input;
use leptos::html::Video;
use leptos::task::spawn_local;
use leptos::{ev::SubmitEvent, prelude::*};
use leptos::logging::log;
use leptos_use::{use_clipboard, UseClipboardReturn};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    DisplayMediaStreamConstraints, HtmlVideoElement, MessageEvent, RtcConfiguration,
    RtcIceCandidateInit, RtcPeerConnection, RtcPeerConnectionIceEvent,
    RtcIceConnectionState, RtcSdpType, RtcSessionDescriptionInit, RtcTrackEvent,
    WebSocket,
};
use uuid::Uuid;

// Read from frontend/.env at compile time.
const SIGNALING_WS_URL: &str = dotenvy_macro::dotenv!("SIGNALING_WS_URL");

thread_local! {
    static ACTIVE_PEER: RefCell<Option<RtcPeerConnection>> = const { RefCell::new(None) };
    static ACTIVE_SOCKET: RefCell<Option<WebSocket>> = const { RefCell::new(None) };
}

#[derive(Serialize, Deserialize)]
struct SignalMessage {
    #[serde(rename = "type")]
    kind: String,
    session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp_mid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp_m_line_index: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    participant_count: Option<usize>,
}

fn send_signal(socket: &WebSocket, message: &SignalMessage) -> Result<(), JsValue> {
    let payload = serde_json::to_string(message)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    socket.send_with_str(&payload)
}

fn create_peer_connection() -> Result<RtcPeerConnection, JsValue> {
    let config = RtcConfiguration::new();
    let ice_servers = js_sys::Array::new();
    let server = js_sys::Object::new();
    let urls = js_sys::Array::new();
    urls.push(&JsValue::from_str("stun:stun.l.google.com:19302"));
    js_sys::Reflect::set(&server, &"urls".into(), &urls)?;
    ice_servers.push(&server);
    config.set_ice_servers(&ice_servers);
    RtcPeerConnection::new_with_configuration(&config)
}

fn stop_media_stream(stream: &web_sys::MediaStream) {
    stream.get_tracks().for_each(&mut |track, _, _| {
        if let Ok(track) = track.dyn_into::<web_sys::MediaStreamTrack>() {
            track.stop();
        }
    });
}

#[component]
pub fn App() -> impl IntoView {
    let (session_id, set_session_id) = signal(String::new());
    let join_id: NodeRef<Input> = NodeRef::new();

    let (status, set_status) = signal(String::new());
    let (diagnostics, set_diagnostics) = signal(Vec::<String>::new());
    let (sharing, set_sharing) = signal(false);
    let (watching, set_watching) = signal(false);
    let (participant_count, set_participant_count) = signal(1usize);
    
    let video_ref: NodeRef<Video> = NodeRef::new();
    let remote_video_ref: NodeRef<Video> = NodeRef::new();

    let start_share = move |_| {
        // Diagnostics belong to the current attempt, not the previous session.
        set_diagnostics.set(Vec::new());
        set_status.set(String::new());
        let window = match web_sys::window() {
            Some(window) => window,
            None => {
                set_status.set("Window API unavailable".to_string());
                return;
            }
        };

        if !window.is_secure_context() {
            set_status.set("Screen capture requires a secure context".to_string());
            return;
        }
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

                    let Some(video) = video_ref.get() else {
                        stream.get_tracks().for_each(&mut |track, _, _| {
                            if let Ok(track) = track.dyn_into::<web_sys::MediaStreamTrack>() {
                                track.stop();
                            }
                        });
                        set_status.set("Screen preview is unavailable".to_string());
                        return;
                    };
                    let id = Uuid::new_v4().to_string();

                    let peer = match create_peer_connection() {
                        Ok(peer) => peer,
                        Err(error) => {
                            stop_media_stream(&stream);
                            set_status.set(format!("Could not create WebRTC peer: {:?}", error));
                            return;
                        }
                    };
                    let remote_description_ready = Rc::new(Cell::new(false));
                    let pending_ice = Rc::new(RefCell::new(Vec::<RtcIceCandidateInit>::new()));
                    // `addStream` is deprecated and has inconsistent support in
                    // WebView2. Add each captured track through the modern API.
                    stream.get_tracks().for_each(&mut |track, _, _| {
                        if let Ok(track) = track.dyn_into::<web_sys::MediaStreamTrack>() {
                            peer.add_track_0(&track, &stream);
                        }
                    });

                    let socket = match WebSocket::new(SIGNALING_WS_URL) {
                        Ok(socket) => socket,
                        Err(error) => {
                            peer.close();
                            stop_media_stream(&stream);
                            set_status.set(format!("Could not connect to signaling server: {:?}", error));
                            return;
                        }
                    };

                    let ice_socket = socket.clone();
                    let ice_session = id.clone();
                    let ice_diagnostics = set_diagnostics;
                    let on_ice = Closure::<dyn FnMut(RtcPeerConnectionIceEvent)>::new(
                        move |event: RtcPeerConnectionIceEvent| {
                            let Some(candidate) = event.candidate() else { return; };
                            ice_diagnostics.update(|entries| entries.push("Local ICE candidate gathered".into()));
                            let message = SignalMessage {
                                kind: "ice-candidate".to_string(),
                                session_id: ice_session.clone(),
                                candidate: Some(candidate.candidate()),
                                sdp_mid: candidate.sdp_mid(),
                                sdp_m_line_index: candidate.sdp_m_line_index(),
                                participant_count: None,
                                sdp: None,
                            };
                            if ice_socket.ready_state() == WebSocket::OPEN {
                                if let Err(error) = send_signal(&ice_socket, &message) {
                                    log!("Could not send ICE candidate: {:?}", error);
                                }
                            }
                        },
                    );
                    peer.set_onicecandidate(Some(on_ice.as_ref().unchecked_ref()));
                    on_ice.forget();

                    let offer_peer = peer.clone();
                    let offer_socket = socket.clone();
                    let offer_session = id.clone();
                    let on_open = Closure::<dyn FnMut()>::new(move || {
                        let peer = offer_peer.clone();
                        let socket = offer_socket.clone();
                        let session = offer_session.clone();
                        spawn_local(async move {
                            let result = async {
                                send_signal(&socket, &SignalMessage {
                                    kind: "create".to_string(),
                                    session_id: session.clone(),
                                    sdp: None,
                                    candidate: None,
                                    sdp_mid: None,
                                    sdp_m_line_index: None,
                                    participant_count: None,
                                })?;
                                let offer_value = JsFuture::from(peer.create_offer()).await?;
                                let offer: RtcSessionDescriptionInit = offer_value.unchecked_into();
                                JsFuture::from(peer.set_local_description(&offer)).await?;
                                let sdp = offer.get_sdp().unwrap_or_default();
                                send_signal(&socket, &SignalMessage {
                                    kind: "offer".to_string(),
                                    session_id: session,
                                    sdp: Some(sdp),
                                    candidate: None,
                                    sdp_mid: None,
                                    sdp_m_line_index: None,
                                    participant_count: None,
                                })?;
                                Ok::<(), JsValue>(())
                            }.await;
                            if let Err(error) = result {
                                log!("Could not create/send WebRTC offer: {:?}", error);
                            }
                        });
                    });
                    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
                    on_open.forget();

                    let message_peer = peer.clone();
                    let message_session = id.clone();
                    let share_diagnostics = set_diagnostics;
                    let remote_description_ready_for_messages = remote_description_ready.clone();
                    let pending_ice_for_messages = pending_ice.clone();
                    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
                        let Some(payload) = event.data().as_string() else { return; };
                        let Ok(message) = serde_json::from_str::<SignalMessage>(&payload) else { return; };
                        if message.session_id != message_session { return; }
                        let peer = message_peer.clone();
                        match message.kind.as_str() {
                            "participants" => {
                                if let Some(count) = message.participant_count {
                                    share_diagnostics.update(|entries| entries.push(format!("Server reports {} participant(s) in this session", count)));
                                    set_participant_count.set(count);
                                }
                            }
                            "answer" => {
                                if let Some(sdp) = message.sdp {
                                    share_diagnostics.update(|entries| entries.push("Received viewer answer".into()));
                                    let ready = remote_description_ready_for_messages.clone();
                                    let pending = pending_ice_for_messages.clone();
                                    spawn_local(async move {
                                        let answer = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                                        answer.set_sdp(&sdp);
                                        match JsFuture::from(peer.set_remote_description(&answer)).await {
                                            Ok(_) => {
                                                ready.set(true);
                                                let candidates = std::mem::take(&mut *pending.borrow_mut());
                                                for candidate in candidates {
                                                    match web_sys::RtcIceCandidate::new(&candidate) {
                                                        Ok(candidate) => if let Err(error) = JsFuture::from(peer.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&candidate))).await {
                                                            log!("Could not apply queued remote ICE candidate: {:?}", error);
                                                        },
                                                        Err(error) => log!("Invalid queued remote ICE candidate: {:?}", error),
                                                    }
                                                }
                                            }
                                            Err(error) => log!("Could not apply remote answer: {:?}", error),
                                        }
                                    });
                                }
                            }
                            "ice-candidate" => {
                                if let Some(candidate) = message.candidate {
                                    let init = RtcIceCandidateInit::new(&candidate);
                                    if let Some(mid) = message.sdp_mid.as_deref() { init.set_sdp_mid(Some(mid)); }
                                    if let Some(index) = message.sdp_m_line_index { init.set_sdp_m_line_index(Some(index)); }
                                    if remote_description_ready_for_messages.get() {
                                        spawn_local(async move {
                                            match web_sys::RtcIceCandidate::new(&init) {
                                                Ok(candidate) => if let Err(error) = JsFuture::from(peer.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&candidate))).await {
                                                    log!("Could not apply remote ICE candidate: {:?}", error);
                                                    share_diagnostics.update(|entries| entries.push(format!("ERROR: Applying remote ICE candidate failed: {:?}", error)));
                                                },
                                                Err(error) => log!("Invalid remote ICE candidate: {:?}", error),
                                            }
                                        });
                                    } else {
                                        pending_ice_for_messages.borrow_mut().push(init);
                                    }
                                }
                            }
                            "error" => {
                                let reason = message.sdp.unwrap_or_else(|| "Unknown signaling error".into());
                                share_diagnostics.update(|entries| entries.push(format!("ERROR: Signaling server: {}", reason)));
                            },
                            _ => {}
                        }
                    });
                    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
                    on_message.forget();

                    ACTIVE_PEER.with(|active| *active.borrow_mut() = Some(peer));
                    ACTIVE_SOCKET.with(|active| *active.borrow_mut() = Some(socket));

                    let video: &HtmlVideoElement = video.unchecked_ref();
                    video.set_src_object(Some(&stream));
                    if let Err(err) = video.play() {
                        log!("Video playback error: {:?}", err);
                    }
                    set_session_id.set(id.clone());
                    set_participant_count.set(1);
                    set_sharing.set(true);
                    set_status.set(format!("Starting share session {}…", id));
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

                    let details = if name == "NotReadableError"
                        && message.to_lowercase().contains("audio")
                    {
                        format!(
                            "{}: {}. The selected window's audio could not be captured. Try sharing a browser tab or the entire screen, and enable the audio option in the share picker.",
                            name, message
                        )
                    } else {
                        format!("{}: {}", name, message)
                    };
                    log!("Screen capture failed: {}", details);
                    set_diagnostics.update(|entries| {
                        entries.push(format!(
                            "ERROR: Screen capture rejected; exception={} ; message={}",
                            name, message
                        ));
                    });
                    set_status.set(details);
                }
            }
        });
    };

    let stop_share = move |_| {
        if let Some(video) = video_ref.get() {
            if let Some(stream) = video.src_object() {
                stop_media_stream(&stream);
                video.set_src_object(None);
            }
        }
        if let Some(video) = remote_video_ref.get() {
            video.set_src_object(None);
        }
        if let Some(socket) = ACTIVE_SOCKET.with(|active| active.borrow_mut().take()) {
            socket.set_onopen(None);
            socket.set_onmessage(None);
            socket.set_onerror(None);
            socket.set_onclose(None);
            let _ = socket.close();
        }
        if let Some(peer) = ACTIVE_PEER.with(|active| active.borrow_mut().take()) {
            peer.set_onicecandidate(None);
            peer.set_ontrack(None);
            peer.close();
        }
        set_watching.set(false);
        set_sharing.set(false);
        set_participant_count.set(1);
        set_status.set("Session closed".to_string());
    };

    let join_session = move |ev: SubmitEvent| {
        ev.prevent_default();
        // Clear stale diagnostics as soon as a new join is attempted, even if
        // validation or peer setup fails.
        set_diagnostics.set(Vec::new());
        set_status.set(String::new());
        log::info!("Join session button clicked");
        let input = join_id
            .get()
            .expect("Join input was not mounted");

        let id = input.value().trim().to_string();

        if id.trim().is_empty() {
            set_status.set("Please enter a session ID.".to_string());
            return;
        }
        set_diagnostics.set(vec![format!("Joining session {}", id)]);

        let peer = match create_peer_connection() {
            Ok(peer) => peer,
            Err(error) => {
                set_diagnostics.update(|entries| entries.push(format!("ERROR: Could not create WebRTC peer: {:?}", error)));
                set_status.set(format!("Could not create WebRTC peer: {:?}", error));
                return;
            }
        };
        let remote_description_ready = Rc::new(Cell::new(false));
        let pending_ice = Rc::new(RefCell::new(Vec::<RtcIceCandidateInit>::new()));
        let socket = match WebSocket::new(SIGNALING_WS_URL) {
            Ok(socket) => socket,
            Err(error) => {
                peer.close();
                set_diagnostics.update(|entries| entries.push(format!("ERROR: Could not open signaling WebSocket: {:?}", error)));
                set_status.set(format!("Could not connect to signaling server: {:?}", error));
                return;
            }
        };

        let ice_socket = socket.clone();
        let ice_session = id.clone();
        let on_ice = Closure::<dyn FnMut(RtcPeerConnectionIceEvent)>::new(move |event: RtcPeerConnectionIceEvent| {
            let Some(candidate) = event.candidate() else { return; };
            if ice_socket.ready_state() != WebSocket::OPEN { return; }
            let result = send_signal(&ice_socket, &SignalMessage {
                kind: "ice-candidate".into(),
                session_id: ice_session.clone(),
                candidate: Some(candidate.candidate()),
                sdp_mid: candidate.sdp_mid(),
                sdp_m_line_index: candidate.sdp_m_line_index(),
                participant_count: None,
                sdp: None,
            });
            if let Err(error) = result { log!("Could not send ICE candidate: {:?}", error); }
        });
        peer.set_onicecandidate(Some(on_ice.as_ref().unchecked_ref()));
        on_ice.forget();

        let join_socket = socket.clone();
        let join_session = id.clone();
        let open_status = set_status;
        let open_diagnostics = set_diagnostics;
        let on_open = Closure::<dyn FnMut()>::new(move || {
            open_diagnostics.update(|entries| entries.push("Signaling WebSocket opened".into()));
            if let Err(error) = send_signal(&join_socket, &SignalMessage {
                kind: "join".into(),
                session_id: join_session.clone(),
                sdp: None,
                candidate: None,
                sdp_mid: None,
                sdp_m_line_index: None,
                participant_count: None,
            }) {
                log!("Could not join signaling room: {:?}", error);
                open_diagnostics.update(|entries| entries.push(format!("ERROR: Could not send join request: {:?}", error)));
                open_status.set("Could not send the join request. Please try again.".to_string());
            } else {
                open_diagnostics.update(|entries| entries.push("Join request sent".into()));
                open_status.set("Request sent. Waiting for the screen sharer…".to_string());
            }
        });
        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();

        let socket_error_status = set_status;
        let socket_error_diagnostics = set_diagnostics;
        let on_socket_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            socket_error_diagnostics.update(|entries| entries.push("ERROR: Signaling WebSocket error".into()));
            socket_error_status.set("Could not reach the signaling server. Check your connection and try again.".to_string());
        });
        socket.set_onerror(Some(on_socket_error.as_ref().unchecked_ref()));
        on_socket_error.forget();

        let socket_close_status = set_status;
        let socket_close_diagnostics = set_diagnostics;
        let on_socket_close = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            socket_close_diagnostics.update(|entries| entries.push("Signaling WebSocket closed".into()));
            socket_close_status.set("The signaling connection closed. You can leave and try again.".to_string());
        });
        socket.set_onclose(Some(on_socket_close.as_ref().unchecked_ref()));
        on_socket_close.forget();

        let incoming_peer = peer.clone();
        let incoming_socket = socket.clone();
        let incoming_session = id.clone();
        let message_diagnostics = set_diagnostics;
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let Some(payload) = event.data().as_string() else { return; };
            let Ok(message) = serde_json::from_str::<SignalMessage>(&payload) else { return; };
            if message.session_id != incoming_session { return; }
            let kind = message.kind.clone();
            message_diagnostics.update(|entries| entries.push(format!("Signaling message: {}", kind)));
            let peer = incoming_peer.clone();
            let socket = incoming_socket.clone();
            match message.kind.as_str() {
                "participants" => {
                    if let Some(count) = message.participant_count {
                        message_diagnostics.update(|entries| entries.push(format!("Server reports {} participant(s)", count)));
                        set_participant_count.set(count);
                    }
                }
                "offer" => {
                    let Some(sdp) = message.sdp else { return; };
                    message_diagnostics.update(|entries| entries.push("Received offer; applying remote description".into()));
                    let session = incoming_session.clone();
                    let ready = remote_description_ready.clone();
                    let pending = pending_ice.clone();
                    spawn_local(async move {
                    let offer = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
                        offer.set_sdp(&sdp);
                        let result = async {
                            JsFuture::from(peer.set_remote_description(&offer)).await?;
                            ready.set(true);
                            let candidates = std::mem::take(&mut *pending.borrow_mut());
                            for candidate in candidates {
                                let candidate = web_sys::RtcIceCandidate::new(&candidate)?;
                                JsFuture::from(peer.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&candidate))).await?;
                            }
                            let answer_value = JsFuture::from(peer.create_answer()).await?;
                            let answer: RtcSessionDescriptionInit = answer_value.unchecked_into();
                            JsFuture::from(peer.set_local_description(&answer)).await?;
                            send_signal(&socket, &SignalMessage {
                                kind: "answer".into(),
                                session_id: session,
                                sdp: answer.get_sdp(),
                                candidate: None,
                                sdp_mid: None,
                                sdp_m_line_index: None,
                                participant_count: None,
                            })?;
                            Ok::<(), JsValue>(())
                        }.await;
                        if let Err(error) = result {
                            log!("Could not create/send WebRTC answer: {:?}", error);
                            message_diagnostics.update(|entries| entries.push(format!("ERROR: Applying offer / sending answer failed: {:?}", error)));
                        } else {
                            message_diagnostics.update(|entries| entries.push("Remote offer applied and answer sent".into()));
                        }
                    });
                }
                "ice-candidate" => if let Some(candidate) = message.candidate {
                    message_diagnostics.update(|entries| entries.push("Received remote ICE candidate".into()));
                    let init = RtcIceCandidateInit::new(&candidate);
                    if let Some(mid) = message.sdp_mid.as_deref() { init.set_sdp_mid(Some(mid)); }
                    if let Some(index) = message.sdp_m_line_index { init.set_sdp_m_line_index(Some(index)); }
                    if remote_description_ready.get() {
                        spawn_local(async move {
                            match web_sys::RtcIceCandidate::new(&init) {
                                Ok(candidate) => if let Err(error) = JsFuture::from(peer.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&candidate))).await {
                                    log!("Could not apply remote ICE candidate: {:?}", error);
                                },
                                Err(error) => log!("Invalid remote ICE candidate: {:?}", error),
                            }
                        });
                    } else {
                        pending_ice.borrow_mut().push(init);
                        message_diagnostics.update(|entries| entries.push("Queued ICE candidate until remote description is ready".into()));
                    }
                },
                "error" => {
                    let reason = message.sdp.unwrap_or_else(|| "Unknown signaling error".into());
                    message_diagnostics.update(|entries| entries.push(format!("ERROR: Signaling server: {}", reason)));
                    set_status.set(format!("Could not join: {}", reason));
                },
                _ => {}
            }
        });
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget();

        let track_status = set_status;
        let track_diagnostics = set_diagnostics;
        let track_session = id.clone();
        let remote_video = remote_video_ref;
        let on_track = Closure::<dyn FnMut(RtcTrackEvent)>::new(move |event: RtcTrackEvent| {
            let track_kind = event.track().kind();
            track_diagnostics.update(|entries| entries.push(format!("Remote {} track received", track_kind)));
            let stream = event.streams().get(0).dyn_into::<web_sys::MediaStream>().ok()
                .or_else(|| {
                    let stream = web_sys::MediaStream::new().ok()?;
                    stream.add_track(&event.track());
                    Some(stream)
                });
            let Some(stream) = stream else {
                track_status.set("Connected, but the remote video track is unavailable.".to_string());
                return;
            };
            if let Some(video) = remote_video.get() {
                let video: &HtmlVideoElement = video.unchecked_ref();
                video.set_src_object(Some(&stream));
                // Keep remote audio enabled so the viewer's player controls
                // can control the shared system audio.
                video.set_muted(false);
                video.set_autoplay(true);
                video.set_attribute("playsinline", "true").ok();
                for event_name in ["loadedmetadata", "playing", "waiting", "stalled", "error"] {
                    let event_diagnostics = track_diagnostics;
                    let event_video = video.clone();
                    let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                        let message = format!(
                            "Video event: {} ({}x{})",
                            event.type_(),
                            event_video.video_width(),
                            event_video.video_height()
                        );
                        if event.type_() == "error" {
                            event_diagnostics.update(|entries| entries.push(format!("ERROR: {}", message)));
                        } else {
                            event_diagnostics.update(|entries| entries.push(message));
                        }
                    });
                    if let Err(error) = video.add_event_listener_with_callback(event_name, callback.as_ref().unchecked_ref()) {
                        track_diagnostics.update(|entries| entries.push(format!("ERROR: Could not observe video {} event: {:?}", event_name, error)));
                    }
                    callback.forget();
                }
                match video.play() {
                    Ok(_) => track_status.set(format!("Connected to session {}", track_session)),
                    Err(error) => {
                        log!("Remote video playback failed: {:?}", error);
                        track_diagnostics.update(|entries| entries.push(format!("ERROR: Video play() failed: {:?}", error)));
                        track_status.set(format!("Connected, but video playback failed: {:?}", error));
                    }
                }
            }
        });
        peer.set_ontrack(Some(on_track.as_ref().unchecked_ref()));
        on_track.forget();

        let connection_status = set_status;
        let connection_diagnostics = set_diagnostics;
        let connection_session = id.clone();
        let connection_peer = peer.clone();
        let on_connection_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let state = connection_peer.ice_connection_state();
            connection_diagnostics.update(|entries| entries.push(format!("ICE connection state: {:?}", state)));
            let message = match state {
                RtcIceConnectionState::Checking => "Connecting to the screen sharer…".to_string(),
                RtcIceConnectionState::Connected | RtcIceConnectionState::Completed => {
                    format!("Connected to session {}", connection_session)
                }
                RtcIceConnectionState::Disconnected => "Connection interrupted. Trying to reconnect…".to_string(),
                RtcIceConnectionState::Failed => "Connection failed. Check the session ID or try again.".to_string(),
                RtcIceConnectionState::Closed => "The screen share has ended.".to_string(),
                RtcIceConnectionState::New => "Waiting for the screen sharer…".to_string(),
                _ => "Connecting to the screen sharer…".to_string(),
            };
            connection_status.set(message);
        });
        peer.set_oniceconnectionstatechange(Some(on_connection_change.as_ref().unchecked_ref()));
        on_connection_change.forget();
        set_diagnostics.update(|entries| entries.push(format!("Initial ICE connection state: {:?}", peer.ice_connection_state())));

        ACTIVE_PEER.with(|active| *active.borrow_mut() = Some(peer));
        ACTIVE_SOCKET.with(|active| *active.borrow_mut() = Some(socket));
        set_watching.set(true);
        set_status.set(format!("Connecting to session {}…", id));
    };

    let UseClipboardReturn { copied, copy, .. } = use_clipboard();

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
                    <img src="/public/cat-knight.svg" alt="Kitten knight holding a sword" />
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
                                <p class="participant-count">
                                    {move || format!("{} connected", participant_count.get())}
                                </p>
                            </div>

                            <div>
                                <button 
                                    class="active-session-button"
                                    on:click=stop_share.clone()>
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
                <form class="join-form" on:submit=join_session>
                    <input
                        node_ref=join_id
                        type="text"
                        placeholder="Paste the session ID"
                        autocomplete="off"
                        autocapitalize="off"
                        spellcheck="false"
                        required=true
                        aria-label="Session ID"
                        disabled=move || watching.get()
                    />

                    <button type="submit" class="join-button" disabled=move || watching.get()>
                        {move || if watching.get() { "Joining…" } else { "Join Screen" }}
                    </button>
                </form>
                {move || if watching.get() {
                    Some(view! {
                        <div class="viewer-card">
                            <div class="viewer-heading">
                                <div>
                                    <span class="viewer-eyebrow">"LIVE SCREEN SHARE"</span>
                                    <h3>"Watching session"</h3>
                                </div>
                                <span class=move || {
                                    let current = status.get();
                                    if current.starts_with("Connected") {
                                        "connection-pill connected"
                                    } else if current.to_lowercase().contains("failed") {
                                        "connection-pill failed"
                                    } else {
                                        "connection-pill"
                                    }
                                }>
                                    <span class="connection-dot"></span>
                                    {move || {
                                        let current = status.get();
                                        if current.starts_with("Connected") {
                                            "Connected"
                                        } else if current.to_lowercase().contains("failed") {
                                            "Failed"
                                        } else {
                                            "Connecting"
                                        }
                                    }}
                                </span>
                            </div>
                            <video
                                node_ref=remote_video_ref
                                autoplay=true
                                playsinline=true
                                controls=true
                                class="remote-video"
                            ></video>
                            <div class="viewer-footer">
                                <p class="viewer-status">{move || status.get()}</p>
                                <button class="leave-button" on:click=stop_share.clone()>
                                    "Leave session"
                                </button>
                            </div>
                        </div>
                    })
                } else { None }}
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

            {move || if !diagnostics.get().is_empty() {
                Some(view! {
                    <details class="diagnostics" open=true>
                        <summary>"Connection diagnostics"</summary>
                        <pre>{move || diagnostics.get().join("\n")}</pre>
                    </details>
                })
            } else { None }}

        </main>
    }
}
