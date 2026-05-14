use krunner_zed::{ZedChannel, channel_from_desktop};

#[test]
fn channel_from_desktop_all_variants() {
    // Name-based detection (primary signal)
    assert_eq!(
        channel_from_desktop("Zed", "/usr/bin/zed"),
        ZedChannel::Stable
    );
    assert_eq!(
        channel_from_desktop("Zed Nightly", "/home/u/.local/zed-nightly.app/bin/zed"),
        ZedChannel::Nightly
    );
    assert_eq!(
        channel_from_desktop("Zed Preview", "/home/u/.local/zed-preview.app/bin/zed"),
        ZedChannel::Preview
    );
    assert_eq!(
        channel_from_desktop("Zed Dev", "/home/u/.local/zed-dev.app/bin/zed"),
        ZedChannel::Dev
    );

    // Exec-path fallback when name is plain "Zed"
    assert_eq!(
        channel_from_desktop("Zed", "/home/u/.local/zed-nightly.app/bin/zed"),
        ZedChannel::Nightly
    );
    assert_eq!(
        channel_from_desktop("Zed", "/home/u/.local/zed-preview.app/bin/zed"),
        ZedChannel::Preview
    );
    assert_eq!(
        channel_from_desktop("Zed", "/home/u/.local/zed-dev.app/bin/zed"),
        ZedChannel::Dev
    );

    // zeditor stable alias — neither name nor exec contain a variant keyword
    assert_eq!(channel_from_desktop("Zed", "zeditor"), ZedChannel::Stable);
}
