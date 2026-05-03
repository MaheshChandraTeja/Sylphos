use std::rc::Rc;

use crate::{
    eval_script_with_research_media, install_cssom_globals, install_dom_globals,
    install_media_globals, install_web_api_globals, CssomHost, DomHost, EventLoopConfig,
    MediaElementKind, MediaHost, ResearchCssomHost, ResearchDom, ResearchMediaHost,
    ResearchWebApiHost, ScheduledVm, VmConfig, WebApiHost,
};

#[test]
fn media_video_element_supports_src_play_pause_and_events() {
    let (summary, _dom, _cssom, media) = eval_script_with_research_media(
        r#"
        const video = document.createElement("video");
        video.id = "player";
        document.body.appendChild(video);

        video.addEventListener("play", function (event) {
            console.log("event", event.type);
        });

        video.src = "sample.mp4";
        video.volume = 0.5;
        video.muted = true;

        video.play().then(function () {
            console.log("played", video.paused, video.readyState, video.volume, video.muted);
        });

        video.pause();
        console.log("paused", video.paused);
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console[0], "paused true");
    assert_eq!(summary.console[1], "played true 2 0.5 true");
    assert_eq!(summary.console[2], "event play");

    let metrics = media.metrics();
    assert_eq!(metrics.src_assignments, 1);
    assert_eq!(metrics.play_calls, 1);
    assert_eq!(metrics.pause_calls, 1);
    assert!(metrics.events >= 3);
}

#[test]
fn mediasource_create_object_url_attach_and_append_buffer() {
    let (summary, _dom, _cssom, media) = eval_script_with_research_media(
        r#"
        const video = document.createElement("video");
        const mediaSource = new MediaSource();

        video.src = URL.createObjectURL(mediaSource);

        const sourceBuffer = mediaSource.addSourceBuffer("video/mp4; codecs=\"avc1.42E01E\"");
        sourceBuffer.appendBuffer("abcdefghij");

        console.log(mediaSource.readyState);
        console.log(mediaSource.sourceBuffers.length);
        console.log(sourceBuffer.buffered.length);
        console.log(sourceBuffer.buffered.end(0));
        console.log(video.buffered.length);
        console.log(video.readyState);

        mediaSource.endOfStream();
        console.log(mediaSource.readyState);
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console[0], "open");
    assert_eq!(summary.console[1], "1");
    assert_eq!(summary.console[2], "1");
    assert_eq!(summary.console[4], "1");
    assert_eq!(summary.console[5], "4");
    assert_eq!(summary.console[6], "ended");

    let metrics = media.metrics();
    assert_eq!(metrics.media_sources_created, 1);
    assert_eq!(metrics.source_buffers_created, 1);
    assert_eq!(metrics.object_urls_created, 1);
    assert_eq!(metrics.media_source_attachments, 1);
    assert_eq!(metrics.buffer_appends, 1);
    assert_eq!(media.segments().len(), 1);
}

#[test]
fn media_can_play_type_and_media_source_is_type_supported_work() {
    let (summary, _dom, _cssom, media) = eval_script_with_research_media(
        r#"
        const video = document.createElement("video");
        console.log(video.canPlayType("video/mp4"));
        console.log(video.canPlayType("application/octet-stream"));
        console.log(MediaSource.isTypeSupported("audio/webm; codecs=\"opus\""));
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["probably", "", "true"]);
    assert_eq!(media.metrics().can_play_type_checks, 3);
}

#[test]
fn media_current_time_and_timeupdate_event_work() {
    let (summary, _dom, _cssom, media) = eval_script_with_research_media(
        r#"
        const audio = document.createElement("audio");
        audio.addEventListener("timeupdate", function (event) {
            console.log("time", audio.currentTime, event.type);
        });

        audio.src = "track.mp4";
        audio.currentTime = 12.5;
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["time 12.5 timeupdate"]);
    assert_eq!(media.metrics().seeks, 1);
}

#[test]
fn media_source_buffer_remove_and_revoke_object_url_work() {
    let (summary, _dom, _cssom, media) = eval_script_with_research_media(
        r#"
        const ms = new MediaSource();
        const url = URL.createObjectURL(ms);
        const sb = ms.addSourceBuffer("video/webm; codecs=\"vp9\"");
        sb.appendBuffer("1234567890");
        console.log(sb.buffered.length);
        sb.remove(0, 100);
        console.log(sb.buffered.length);
        console.log(URL.revokeObjectURL(url));
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["1", "0", "true"]);
    assert_eq!(media.metrics().buffer_appends, 1);
    assert_eq!(media.metrics().buffer_removes, 1);
    assert_eq!(media.metrics().object_urls_revoked, 1);
}

#[test]
fn media_installs_with_dom_cssom_and_webapi_together() {
    let cssom = Rc::new(ResearchCssomHost::default());
    let dom = Rc::new(ResearchDom::with_cssom("Media", cssom.clone()));
    let media = Rc::new(ResearchMediaHost::default());
    let web = Rc::new(ResearchWebApiHost::default());

    let mut scheduled = ScheduledVm::with_config(VmConfig::default(), EventLoopConfig::default());

    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom.clone(),
    );
    install_web_api_globals(&mut scheduled.vm, scheduled.event_loop.clone(), web.clone());
    install_media_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        media.clone(),
        Some(dom.clone()),
    );

    scheduled
        .execute_script(
            r#"
            const video = document.createElement("video");
            video.id = "hero-video";
            video.style.width = "640px";
            localStorage.setItem("videoId", video.id);
            document.body.appendChild(video);
            console.log(localStorage.getItem("videoId"));
            console.log(getComputedStyle(video).width);
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["hero-video", "640px"]);
    assert!(dom.get_element_by_id("hero-video").is_some());
    assert_eq!(media.metrics().media_elements_created, 1);
    assert_eq!(web.metrics().storage_writes, 1);
    assert!(cssom.metrics().inline_writes >= 1);
}

#[test]
fn media_host_can_create_element_without_dom() {
    let media = ResearchMediaHost::default();
    let id = media.create_media_element(MediaElementKind::Video);
    let snapshot = media.element_snapshot(id).expect("snapshot");

    assert_eq!(snapshot.kind, MediaElementKind::Video);
    assert!(snapshot.paused);
}
