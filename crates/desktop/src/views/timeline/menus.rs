//! Popover context menus for tracks, clips, and the add-track row.

use crate::state::Store;
use miniter_domain::{Clip, Timestamp, Track, TrackId, TrackKind};
use miniter_usecases::EditCommand;
use repose_core::{prelude::theme, signal::{Signal, signal}, CursorIcon, Modifier, Vec2, View};
use repose_material::{
    Icon,
    material3::{
        DropdownMenu, DropdownMenuConfig, DropdownMenuEntry, DropdownMenuItem, MenuState,
    },
};
use repose_ui::{Box, Text, TextStyle, overlay::OverlayHandle};
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
    let t_count = store
        .state
        .timeline
        .get()
        .as_ref()
        .map(|tl| tl.tracks.iter().filter(|t| t.kind == TrackKind::Text).count())
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
        DropdownMenuEntry::Item(
            DropdownMenuItem::new(format!("Text Track (T{t_count})"), {
                let store = store.clone();
                let name = format!("T{t_count}");
                move || {
                    store.dispatch_edit(EditCommand::AddTrack {
                        kind: TrackKind::Text,
                        name: name.clone(),
                    });
                }
            })
            .leading_icon(icon_view(Icons::text_fields)),
        ),
    ]
}

/// Dropdown state for each top-level menu bar entry, so the top menus are
/// real (clicking them opens a menu with working actions) instead of inert
/// labels.
#[derive(Clone)]
pub struct TopMenus {
    pub file: Rc<MenuState>,
    pub edit: Rc<MenuState>,
    pub clip: Rc<MenuState>,
    pub sequence: Rc<MenuState>,
    pub marker: Rc<MenuState>,
    pub window: Rc<MenuState>,
    pub help: Rc<MenuState>,
}

impl Default for TopMenus {
    fn default() -> Self {
        Self::new()
    }
}

impl TopMenus {
    pub fn new() -> Self {
        Self {
            file: Rc::new(MenuState::new()),
            edit: Rc::new(MenuState::new()),
            clip: Rc::new(MenuState::new()),
            sequence: Rc::new(MenuState::new()),
            marker: Rc::new(MenuState::new()),
            window: Rc::new(MenuState::new()),
            help: Rc::new(MenuState::new()),
        }
    }
}

/// A top menu bar entry: a clickable label that opens a DropdownMenu anchored
/// to its own global rect (no panel_origin needed).
pub fn top_menu_dropdown(
    store: &Store,
    label: &str,
    state: Rc<MenuState>,
    items: Vec<DropdownMenuEntry>,
) -> View {
    let th = theme();
    let state_for_click = state.clone();
    let trigger = Text(label)
        .size(12.0)
        .color(th.on_surface)
        .modifier(
            Modifier::new()
                .padding_values(repose_core::PaddingValues {
                    left: 8.0,
                    right: 8.0,
                    top: 4.0,
                    bottom: 4.0,
                })
                .clickable()
                .cursor(CursorIcon::Pointer)
                .on_pointer_down(move |_| state_for_click.open()),
        );

    DropdownMenu(
        state,
        store.overlay.clone(),
        Modifier::new(),
        trigger,
        items,
        DropdownMenuConfig::default(),
    )
}

pub fn file_menu_items(store: &Store) -> Vec<DropdownMenuEntry> {
    vec![
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("New Project", {
                let store = store.clone();
                move || {
                    if super::super::editor::confirm_discard_pub(&store) {
                        store.dispatch_project(crate::views::editor::project_command_create());
                    }
                }
            })
            .leading_icon(icon_view(Icons::add)),
        ),
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Open Project…", {
                let store = store.clone();
                move || {
                    if super::super::editor::confirm_discard_pub(&store) {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            store.dispatch_project(crate::views::editor::project_command_open(path));
                        }
                    }
                }
            })
            .leading_icon(icon_view(Icons::folder_open)),
        ),
        DropdownMenuEntry::Divider,
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Import Media…", {
                let store = store.clone();
                move || {
                    if let Some(paths) = rfd::FileDialog::new().pick_files() {
                        store.dispatch_asset(crate::views::editor::asset_command_import(paths));
                    }
                }
            })
            .leading_icon(icon_view(Icons::upload)),
        ),
        DropdownMenuEntry::Divider,
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Save", {
                let store = store.clone();
                move || {
                    store.dispatch_project(crate::views::editor::project_command_save(&store));
                }
            })
            .leading_icon(icon_view(Icons::save)),
        ),
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Save As…", {
                let store = store.clone();
                move || {
                    if let Some(cmd) = crate::views::editor::project_command_save_as(&store) {
                        store.dispatch_project(cmd);
                    }
                }
            })
            .leading_icon(icon_view(Icons::save)),
        ),
    ]
}

pub fn edit_menu_items(store: &Store) -> Vec<DropdownMenuEntry> {
    vec![
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Undo", {
                let store = store.clone();
                move || store.dispatch_undo()
            })
            .leading_icon(icon_view(Icons::undo)),
        ),
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Redo", {
                let store = store.clone();
                move || store.dispatch_redo()
            })
            .leading_icon(icon_view(Icons::redo)),
        ),
        DropdownMenuEntry::Divider,
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Cut", {
                let store = store.clone();
                move || store.cut_selected_clip()
            })
            .leading_icon(icon_view(Icons::content_cut)),
        ),
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Copy", {
                let store = store.clone();
                move || store.copy_selected_clip()
            })
            .leading_icon(icon_view(Icons::content_copy)),
        ),
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Paste", {
                let store = store.clone();
                move || store.paste_clip()
            })
            .leading_icon(icon_view(Icons::content_paste)),
        ),
    ]
}

pub fn clip_top_menu_items(store: &Store) -> Vec<DropdownMenuEntry> {
    let Some(clip_id) = store.state.selected_clip_id.get() else {
        return vec![DropdownMenuEntry::Item(
            DropdownMenuItem::new("No clip selected", || {})
                .disabled(),
        )];
    };

    let split_enabled = store
        .state
        .timeline
        .get()
        .as_ref()
        .map(|tl| super::can_split_clip(tl, clip_id, store.state.playhead.get()))
        .unwrap_or(false);

    vec![
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Split at Playhead", {
                let store = store.clone();
                move || {
                    let at = store.state.playhead.get();
                    let Some(timeline) = store.state.timeline.get() else { return };
                    if !super::can_split_clip(&timeline, clip_id, at) {
                        return;
                    }
                    store.dispatch_edit(EditCommand::SplitClip {
                        clip_id,
                        at,
                        new_clip_id: miniter_domain::ClipId::new(),
                    });
                }
            })
            .leading_icon(icon_view(Icons::content_cut))
            .let_enabled(split_enabled),
        ),
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Duplicate", {
                let store = store.clone();
                move || {
                    let Some(timeline) = store.state.timeline.get() else {
                        return;
                    };
                    let Some((clip, track)) = timeline
                        .tracks
                        .iter()
                        .find_map(|t| t.clip_by_id(clip_id).map(|c| (c, t)))
                    else {
                        return;
                    };
                    store.dispatch_edit(EditCommand::DuplicateClip {
                        source_clip_id: clip_id,
                        new_clip_id: miniter_domain::ClipId::new(),
                        target_track_id: track.id,
                        target_start: clip.timeline_end(),
                    });
                }
            })
            .leading_icon(icon_view(Icons::layers)),
        ),
        DropdownMenuEntry::Divider,
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Delete", {
                let store = store.clone();
                move || {
                    store.dispatch_edit(EditCommand::RemoveClip { clip_id });
                    store.state.selected_clip_id.set(None);
                }
            })
            .leading_icon(icon_view(Icons::delete)),
        ),
    ]
}

pub fn sequence_menu_items(store: &Store) -> Vec<DropdownMenuEntry> {
    add_track_menu_items(store)
}

pub fn marker_menu_items(store: &Store) -> Vec<DropdownMenuEntry> {
    let playhead = store.state.playhead.get().0;
    vec![
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Add Marker at Playhead", {
                let store = store.clone();
                let ts = playhead;
                move || {
                    let mut list = store.state.timeline_markers.get();
                    if !list.iter().any(|m| m.timestamp_us == ts) {
                        list.push(crate::state::TimelineMarker {
                            timestamp_us: ts,
                            label: format!("M{}", list.len() + 1),
                        });
                        store.state.timeline_markers.set(list);
                    }
                }
            })
            .leading_icon(icon_view(Icons::bookmark_add)),
        ),
        DropdownMenuEntry::Item(
            DropdownMenuItem::new("Clear All Markers", {
                let store = store.clone();
                move || {
                    store.state.timeline_markers.set(Vec::new());
                }
            })
            .leading_icon(icon_view(Icons::bookmark_remove)),
        ),
    ]
}

pub fn window_menu_items(store: &Store) -> Vec<DropdownMenuEntry> {
    vec![DropdownMenuEntry::Item(
        DropdownMenuItem::new("Reset Layout", {
            let store = store.clone();
            move || {
                *store.dock_state.borrow_mut() =
                    crate::views::panels::create_default_layout();
            }
        })
        .leading_icon(icon_view(Icons::view_quilt)),
    )]
}

pub fn help_menu_items() -> Vec<DropdownMenuEntry> {
    vec![DropdownMenuEntry::Item(
        DropdownMenuItem::new(
            "Snapshort",
            || {
                let _ = rfd::MessageDialog::new()
                    .set_title("About Snapshort")
                    .set_description("Snapshort — a Miniter-based timeline editor.")
                    .set_buttons(rfd::MessageButtons::Ok)
                    .show();
            },
        )
        .leading_icon(icon_view(Icons::info)),
    )]
}

trait LetEnabled: Sized {
    fn let_enabled(self, enabled: bool) -> Self;
}

impl LetEnabled for DropdownMenuItem {
    fn let_enabled(self, enabled: bool) -> Self {
        if enabled {
            self
        } else {
            self.disabled()
        }
    }
}