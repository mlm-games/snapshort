//! Popover context menus for tracks, clips, and the add-track row.

use crate::state::Store;
use miniter_domain::{Clip, Timestamp, Track, TrackId, TrackKind};
use miniter_usecases::EditCommand;
use repose_core::{Modifier, Vec2, View, prelude::theme, signal::{Signal, signal}};
use repose_material::{Icon, material3::{
    DropdownMenu, DropdownMenuConfig, DropdownMenuEntry, DropdownMenuItem, MenuState,
}};
use repose_ui::{Box, TextStyle, overlay::OverlayHandle};
use snapshort_ui_core::Icons;
use snapshort_usecases::PlaybackCommand;
use std::rc::Rc;

/// Anchor state for a single popover menu.
#[derive(Clone)]
pub struct MenuTarget {
    pub state: Rc<MenuState>,
    pub local_anchor: Signal<Option<Vec2>>,
}

impl MenuTarget {
    pub fn new() -> Self {
        Self {
            state: Rc::new(MenuState::new()),
            local_anchor: signal(None),
        }
    }

    /// Open the menu anchored at a window position. `panel_origin` is the
    /// timeline panel's global top-left, captured via `on_globally_positioned`.
    pub fn open_at_window(&self, window_pos: Vec2, panel_origin: Vec2) {
        self.local_anchor.set(Some(window_pos - panel_origin));
        self.state.open();
    }
}

impl Default for MenuTarget {
    fn default() -> Self {
        Self::new()
    }
}

/// Render the anchored dropdown for `target`.
pub fn popover_view(
    overlay: OverlayHandle,
    target: &MenuTarget,
    items: Vec<DropdownMenuEntry>,
) -> View {
    let anchor = target.local_anchor.get().unwrap_or(Vec2::ZERO);
    let state = target.state.clone();

    let trigger = Box(Modifier::new()
        .width(1.0)
        .height(1.0)
        .absolute()
        .offset(Some(anchor.x), Some(anchor.y), None, None)
        .z_index(1.0));

    DropdownMenu(
        state,
        overlay,
        Modifier::new(),
        trigger,
        items,
        DropdownMenuConfig::default(),
    )
}

fn icon_view(symbol: repose_material::Symbol) -> View {
    let th = theme();
    Icon(symbol).size(16.0).color(th.on_surface_variant)
}

pub fn clip_menu_items(
    store: &Store,
    clip: &Clip,
    track_id: TrackId,
) -> Vec<DropdownMenuEntry> {
    let clip_id = clip.id;
    let playhead_us = store.state.playhead.get().0;
    let clip_start = clip.timeline_start.0;
    let clip_end = clip.timeline_end().as_micros();
    let split_enabled = playhead_us > clip_start && playhead_us < clip_end;

    let mut items: Vec<DropdownMenuEntry> = Vec::new();

    items.push(DropdownMenuEntry::Item(
        DropdownMenuItem::new("Set Playhead Here", {
            let store = store.clone();
            let ts = clip.timeline_start;
            move || {
                store.dispatch_playback(PlaybackCommand::Seek { timestamp: ts });
            }
        })
        .leading_icon(icon_view(Icons::play_arrow)),
    ));

    if split_enabled {
        items.push(DropdownMenuEntry::Item(
            DropdownMenuItem::new("Split at Playhead", {
                let store = store.clone();
                let at = Timestamp(playhead_us);
                    move || {
                    store.dispatch_edit(EditCommand::SplitClip {
                        clip_id,
                        at,
                        new_clip_id: miniter_domain::ClipId::new(),
                    });
                }
            })
            .leading_icon(icon_view(Icons::content_cut)),
        ));
    };

    items.push(DropdownMenuEntry::Item(
        DropdownMenuItem::new("Duplicate", {
            let store = store.clone();
            let target_track_id = track_id;
            let target_start = clip.timeline_end();
            move || {
                store.dispatch_edit(EditCommand::DuplicateClip {
                    source_clip_id: clip_id,
                    new_clip_id: miniter_domain::ClipId::new(),
                    target_track_id,
                    target_start,
                });
            }
        })
        .leading_icon(icon_view(Icons::layers)),
    ));

    items.push(DropdownMenuEntry::Divider);

    items.push(DropdownMenuEntry::Item(
        DropdownMenuItem::new(if clip.muted { "Unmute" } else { "Mute" }, {
            let store = store.clone();
            let muted = !clip.muted;
            move || {
                store.dispatch_edit(EditCommand::SetClipMuted { clip_id, muted });
            }
        })
        .leading_icon(icon_view(if clip.muted {
            Icons::volume_up
        } else {
            Icons::volume_off
        })),
    ));

    items.push(DropdownMenuEntry::Item(
        DropdownMenuItem::new("Delete", {
            let store = store.clone();
            move || {
                store.dispatch_edit(EditCommand::RemoveClip { clip_id });
                if store.state.selected_clip_id.get() == Some(clip_id) {
                    store.state.selected_clip_id.set(None);
                }
            }
        })
        .leading_icon(icon_view(Icons::delete)),
    ));

    items
}

pub fn track_menu_items(store: &Store, track: &Track) -> Vec<DropdownMenuEntry> {
    let track_id = track.id;
    let timeline = store.state.timeline.get();
    let is_last_video = matches!(track.kind, TrackKind::Video)
        && timeline
            .as_ref()
            .map(|tl| tl.tracks.iter().filter(|t| t.kind == TrackKind::Video).count() <= 1)
            .unwrap_or(true);

    let mut items: Vec<DropdownMenuEntry> = Vec::new();

    items.push(DropdownMenuEntry::Item(
        DropdownMenuItem::new(if track.muted { "Unmute" } else { "Mute" }, {
            let store = store.clone();
            let muted = !track.muted;
            move || {
                store.dispatch_edit(EditCommand::SetTrackMuted { track_id, muted });
            }
        })
        .leading_icon(icon_view(if track.muted {
            Icons::volume_up
        } else {
            Icons::volume_off
        })),
    ));

    items.push(DropdownMenuEntry::Item(
        DropdownMenuItem::new(if track.locked { "Unlock" } else { "Lock" }, {
            let store = store.clone();
            let locked = !track.locked;
            move || {
                store.dispatch_edit(EditCommand::SetTrackLocked { track_id, locked });
            }
        })
        .leading_icon(icon_view(if track.locked {
            Icons::lock_open
        } else {
            Icons::lock
        })),
    ));

    items.push(DropdownMenuEntry::Divider);

    if !is_last_video {
        items.push(DropdownMenuEntry::Item(
            DropdownMenuItem::new("Remove Track", {
                let store = store.clone();
                    move || {
                    store.dispatch_edit(EditCommand::RemoveTrack { track_id });
                }
            })
            .leading_icon(icon_view(Icons::delete)),
        ));
    }

    items
}

pub fn add_track_menu_items(store: &Store) -> Vec<DropdownMenuEntry> {
    let v_count = store
        .state
        .timeline
        .get()
        .as_ref()
        .map(|tl| tl.tracks.iter().filter(|t| t.kind == TrackKind::Video).count())
        .unwrap_or(0)
        + 1;
    let a_count = store
        .state
        .timeline
        .get()
        .as_ref()
        .map(|tl| tl.tracks.iter().filter(|t| t.kind == TrackKind::Audio).count())
        .unwrap_or(0)
        + 1;

    vec![
        DropdownMenuEntry::Item(
            DropdownMenuItem::new(format!("Video Track (V{v_count})"), {
                let store = store.clone();
                let name = format!("V{v_count}");
                move || {
                    store.dispatch_edit(EditCommand::AddTrack {
                        kind: TrackKind::Video,
                        name: name.clone(),
                    });
                }
            })
            .leading_icon(icon_view(Icons::movie)),
        ),
        DropdownMenuEntry::Item(
            DropdownMenuItem::new(format!("Audio Track (A{a_count})"), {
                let store = store.clone();
                let name = format!("A{a_count}");
                move || {
                    store.dispatch_edit(EditCommand::AddTrack {
                        kind: TrackKind::Audio,
                        name: name.clone(),
                    });
                }
            })
            .leading_icon(icon_view(Icons::music_note)),
        ),
    ]
}