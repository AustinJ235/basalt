use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use flume::{Receiver, Sender};
use foldhash::{HashMap, HashMapExt};

use crate::input::state::WindowState;
use crate::input::{Hook, InputEvent, InputHookID, proc};
use crate::interface::{BinID, Interface};
use crate::interval::Interval;
use crate::window::WindowID;

pub(in crate::input) enum LoopEvent {
    Normal(InputEvent),
    Add { id: InputHookID, hook: Hook },
    FocusBin { win: WindowID, bin: Option<BinID> },
    SmoothScroll { win: WindowID, v: f32, h: f32 },
    Remove(InputHookID),
}

pub(in crate::input) fn begin_loop(
    interface: Arc<Interface>,
    interval: Arc<Interval>,
    event_send: Sender<LoopEvent>,
    event_recv: Receiver<LoopEvent>,
) {
    thread::spawn(move || {
        let mut hooks: HashMap<InputHookID, Hook> = HashMap::new();
        let mut win_state: HashMap<WindowID, WindowState> = HashMap::new();
        let (ss_send, ss_recv) = flume::unbounded();

        struct SmoothScroll {
            step: f32,
            rem: [f32; 2],
            amt: [f32; 2],
            cycles: [u16; 2],
        }

        let mut ss_state: HashMap<WindowID, SmoothScroll> = HashMap::new();
        const SS_CYCLES: u16 = 20;

        // TODO: Configure frequency of output?
        interval.start(interval.do_every(Duration::from_millis(8), None, move |_| {
            while let Ok((win, v, h)) = ss_recv.try_recv() {
                let state = ss_state.entry(win).or_insert_with(|| {
                    SmoothScroll {
                        step: 100.0,
                        rem: [0.0; 2],
                        amt: [0.0; 2],
                        cycles: [0; 2],
                    }
                });

                if v != 0.0 {
                    let accel = ((state.rem[0].abs() / state.step) / 1.5).clamp(1.0, 4.0);
                    state.rem[0] += v * state.step * accel;
                    state.amt[0] = state.rem[0];
                    state.cycles[0] = SS_CYCLES;
                }

                if h != 0.0 {
                    let accel = ((state.rem[1].abs() / state.step) / 1.5).clamp(1.0, 4.0);
                    state.rem[1] += h * state.step * accel;
                    state.amt[1] = state.rem[1];
                    state.cycles[1] = SS_CYCLES;
                }
            }

            for (win, state) in ss_state.iter_mut() {
                let v = if state.cycles[0] != 0 {
                    let amt =
                        state.amt[0] * ((state.cycles[0] as f32 - 0.5) / (SS_CYCLES as f32 * 10.0));
                    state.rem[0] -= amt;
                    state.cycles[0] -= 1;

                    if state.cycles[0] == 0 {
                        state.rem[0] = 0.0;
                    }

                    amt
                } else {
                    0.0
                };

                let h = if state.cycles[1] != 0 {
                    let amt =
                        state.amt[1] * ((state.cycles[1] as f32 - 0.5) / (SS_CYCLES as f32 * 10.0));
                    state.rem[1] -= amt;
                    state.cycles[1] -= 1;

                    if state.cycles[1] == 0 {
                        state.rem[1] = 0.0;
                    }

                    amt
                } else {
                    0.0
                };

                if v != 0.0 || h != 0.0 {
                    event_send
                        .send(LoopEvent::SmoothScroll {
                            win: *win,
                            v,
                            h,
                        })
                        .unwrap();
                }
            }

            Default::default()
        }));

        let mut cursor_event_last: BTreeMap<WindowID, [f32; 2]> = BTreeMap::new();
        let mut motion_event_sum: Option<[f32; 2]> = None;
        let mut pending_events: Vec<LoopEvent> = Vec::new();

        loop {
            pending_events.extend(event_recv.drain());

            for event in pending_events.drain(..) {
                if !matches!(
                    event,
                    LoopEvent::Normal(InputEvent::Cursor { .. })
                        | LoopEvent::Normal(InputEvent::Motion { .. })
                ) {
                    proc_cursor_and_motion(
                        &interface,
                        &mut hooks,
                        &mut win_state,
                        &mut cursor_event_last,
                        motion_event_sum.take(),
                    );
                }

                match event {
                    LoopEvent::Add {
                        id,
                        hook,
                    } => {
                        hooks.insert(id, hook);
                    },
                    LoopEvent::Remove(id) => {
                        hooks.remove(&id);
                    },
                    LoopEvent::FocusBin {
                        win,
                        bin,
                    } => {
                        let window_state = win_state
                            .entry(win)
                            .or_insert_with(|| WindowState::new(win));

                        if let Some((old_bin_id_op, new_bin_id_op)) =
                            window_state.update_focus_bin(bin)
                        {
                            proc::bin_focus(
                                &interval,
                                &mut hooks,
                                window_state,
                                old_bin_id_op,
                                new_bin_id_op,
                            );
                        }
                    },
                    LoopEvent::SmoothScroll {
                        win,
                        v,
                        h,
                    } => {
                        proc::scroll(&interface, &mut hooks, &mut win_state, win, true, v, h);
                    },
                    LoopEvent::Normal(event) => {
                        match event {
                            InputEvent::Press {
                                win,
                                key,
                            } => {
                                proc::press(
                                    &interface,
                                    &interval,
                                    &mut hooks,
                                    &mut win_state,
                                    win,
                                    key,
                                );
                            },
                            InputEvent::Release {
                                win,
                                key,
                            } => {
                                proc::release(&interval, &mut hooks, &mut win_state, win, key);
                            },
                            InputEvent::Character {
                                win,
                                c,
                            } => {
                                proc::character(&mut hooks, &mut win_state, win, c);
                            },
                            InputEvent::Focus {
                                win,
                            } => {
                                proc::window_focus(&mut hooks, &mut win_state, win, true);
                            },
                            InputEvent::FocusLost {
                                win,
                            } => {
                                proc::window_focus(&mut hooks, &mut win_state, win, false);
                            },
                            InputEvent::Cursor {
                                win,
                                x,
                                y,
                            } => {
                                cursor_event_last.insert(win, [x, y]);
                            },
                            InputEvent::Scroll {
                                win,
                                v,
                                h,
                            } => {
                                ss_send.send((win, v, h)).unwrap();
                                proc::scroll(
                                    &interface,
                                    &mut hooks,
                                    &mut win_state,
                                    win,
                                    false,
                                    v,
                                    h,
                                );
                            },
                            InputEvent::Enter {
                                win,
                            } => {
                                proc::window_cursor_inside(&mut hooks, &mut win_state, win, true);
                            },
                            InputEvent::Leave {
                                win,
                            } => {
                                proc::window_cursor_inside(&mut hooks, &mut win_state, win, false);
                            },
                            InputEvent::Motion {
                                x,
                                y,
                            } => {
                                match motion_event_sum.as_mut() {
                                    Some(sum) => {
                                        sum[0] += x;
                                        sum[1] += y;
                                    },
                                    None => {
                                        motion_event_sum = Some([x, y]);
                                    },
                                }
                            },
                            InputEvent::CursorCapture {
                                win,
                                captured,
                            } => {
                                let window_state = win_state
                                    .entry(win)
                                    .or_insert_with(|| WindowState::new(win));

                                if window_state.update_cursor_captured(captured) {
                                    if captured {
                                        if let Some((old_bin_id_op, ..)) =
                                            window_state.update_focus_bin(None)
                                        {
                                            proc::bin_focus(
                                                &interval,
                                                &mut hooks,
                                                window_state,
                                                old_bin_id_op,
                                                None,
                                            );
                                        }
                                    }

                                    let [x, y] = window_state.cursor_pos();

                                    proc::cursor(
                                        &interface,
                                        &mut hooks,
                                        &mut win_state,
                                        win,
                                        x,
                                        y,
                                        true,
                                    );
                                }
                            },
                        }
                    },
                }
            }

            proc_cursor_and_motion(
                &interface,
                &mut hooks,
                &mut win_state,
                &mut cursor_event_last,
                motion_event_sum.take(),
            );

            match event_recv.recv() {
                Ok(event) => pending_events.push(event),
                Err(_) => break,
            }
        }
    });
}

fn proc_cursor_and_motion(
    interface: &Arc<Interface>,
    hooks: &mut HashMap<InputHookID, Hook>,
    win_state: &mut HashMap<WindowID, WindowState>,
    cursor_event_last: &mut BTreeMap<WindowID, [f32; 2]>,
    motion_event_sum: Option<[f32; 2]>,
) {
    for (win, [x, y]) in cursor_event_last.iter() {
        proc::cursor(interface, hooks, win_state, *win, *x, *y, false);
    }

    cursor_event_last.clear();

    if let Some([x, y]) = motion_event_sum {
        proc::motion(hooks, x, y);
    }
}
