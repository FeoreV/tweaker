use capi::sctypes::*;
use capi::scbehavior::*;
use capi::scdom::{HELEMENT};
use value::Value;
use dom::event::{EventHandler, EventReason};

fn edit_event_reason(reason: UINT_PTR) -> EventReason {
	match reason {
		0 => EventReason::EditChanged(EDIT_CHANGED_REASON::BY_INS_CHAR),
		1 => EventReason::EditChanged(EDIT_CHANGED_REASON::BY_INS_CHARS),
		2 => EventReason::EditChanged(EDIT_CHANGED_REASON::BY_DEL_CHAR),
		3 => EventReason::EditChanged(EDIT_CHANGED_REASON::BY_DEL_CHARS),
		4 => EventReason::EditChanged(EDIT_CHANGED_REASON::BY_UNDO_REDO),
		5 => EventReason::EditChanged(EDIT_CHANGED_REASON::CHANGE_BY_INS_CONSECUTIVE_CHAR),
		6 => EventReason::EditChanged(EDIT_CHANGED_REASON::CHANGE_BY_DEL_CONSECUTIVE_CHAR),
		7 => EventReason::EditChanged(EDIT_CHANGED_REASON::CHANGE_BY_CODE),
		_ => EventReason::Unknown(reason),
	}
}

fn general_event_reason(reason: UINT_PTR) -> EventReason {
	match reason {
		0 => EventReason::General(CLICK_REASON::BY_MOUSE_CLICK),
		1 => EventReason::General(CLICK_REASON::BY_KEY_CLICK),
		2 => EventReason::General(CLICK_REASON::SYNTHESIZED),
		3 => EventReason::General(CLICK_REASON::BY_MOUSE_ON_ICON),
		_ => EventReason::Unknown(reason),
	}
}

fn decode_phase(raw: UINT) -> Option<PHASE_MASK> {
	match raw {
		0 => Some(PHASE_MASK::BUBBLING),
		x if x == PHASE_MASK::SINKING as UINT => Some(PHASE_MASK::SINKING),
		x if x == PHASE_MASK::BUBBLING_HANDLED as UINT => Some(PHASE_MASK::BUBBLING_HANDLED),
		x if x == PHASE_MASK::SINKING_HANDLED as UINT => Some(PHASE_MASK::SINKING_HANDLED),
		_ => None,
	}
}

fn decode_draw_layer(raw: UINT) -> Option<DRAW_EVENTS> {
	match raw {
		0 => Some(DRAW_EVENTS::DRAW_BACKGROUND),
		1 => Some(DRAW_EVENTS::DRAW_CONTENT),
		2 => Some(DRAW_EVENTS::DRAW_FOREGROUND),
		3 => Some(DRAW_EVENTS::DRAW_OUTLINE),
		_ => None,
	}
}

#[repr(C)]
pub(crate) struct WindowHandler<T>
{
	pub hwnd: HWINDOW,
	pub handler: T,
}

#[repr(C)]
pub(crate) struct BoxedHandler {
	pub handler: Box<dyn EventHandler>,
}

fn is_detach_event(evtg: UINT, params: LPVOID) -> bool {
	let evtg = EVENT_GROUPS(evtg);
	if evtg == EVENT_GROUPS::HANDLE_INITIALIZATION && !params.is_null() {
		let scnm = params as *const UINT;
		let cmd = unsafe { *scnm };
		if cmd == INITIALIZATION_EVENTS::BEHAVIOR_DETACH as UINT {
			return true;
		}
	}
	false
}

pub(crate) extern "system" fn _event_handler_window_proc<T: EventHandler>(tag: LPVOID, _he: ::capi::scdom::HELEMENT, evtg: UINT, params: LPVOID) -> BOOL
{
	let boxed = tag as *mut WindowHandler<T>;
	let tuple: &mut WindowHandler<T> = unsafe { &mut *boxed };

	let hroot: HELEMENT = if let Ok(root) = unsafe { ::dom::Element::from_window(tuple.hwnd) } {
		root.as_ptr()
	} else {
		::std::ptr::null_mut()
	};

	// custom initialization (because there is no DOM in plain window)
	if is_detach_event(evtg, params) {
		tuple.handler.detached(hroot);

		// here we drop our tuple
		let ptr = unsafe { Box::from_raw(boxed) };
		drop(ptr);

		return true as BOOL;
	}

	process_events(&mut tuple.handler, hroot, evtg, params)
}

pub(crate) extern "system" fn _event_handler_behavior_proc(tag: LPVOID, he: HELEMENT, evtg: UINT, params: LPVOID) -> BOOL {
	// reconstruct pointer to Handler
	let boxed = tag as *mut BoxedHandler;
	let me = unsafe { &mut *boxed };
	let me = &mut *me.handler;

	if is_detach_event(evtg, params) {
		me.detached(he);

		// here we drop our handler
		let ptr = unsafe { Box::from_raw(boxed) };
		drop(ptr);

		return true as BOOL;
	}

	process_events(me, he, evtg, params)
}

pub(crate) extern "system" fn _event_handler_proc<T: EventHandler>(tag: LPVOID, he: HELEMENT, evtg: UINT, params: LPVOID) -> BOOL
{
	// reconstruct pointer to Handler
	let boxed = tag as *mut T;
	let me = unsafe { &mut *boxed };

	if is_detach_event(evtg, params) {
		me.detached(he);

		// here we drop our handler
		let ptr = unsafe { Box::from_raw(boxed) };
		drop(ptr);

		return true as BOOL;
	}

	process_events(me, he, evtg, params)
}

fn process_events(me: &mut dyn EventHandler, he: HELEMENT, evtg: UINT, params: LPVOID) -> BOOL
{
	let evtg = EVENT_GROUPS(evtg);
	if he.is_null()
		&& evtg != EVENT_GROUPS::SUBSCRIPTIONS_REQUEST
		&& evtg != EVENT_GROUPS::HANDLE_BEHAVIOR_EVENT
		&& evtg != EVENT_GROUPS::HANDLE_INITIALIZATION
		&& evtg != EVENT_GROUPS::HANDLE_SOM
	{
		eprintln!("[sciter] warning! null element for {:04X}", evtg.0 as u32);
	}

	let result = match evtg {

		EVENT_GROUPS::SUBSCRIPTIONS_REQUEST => {
			if params.is_null() { return false as BOOL; }
			let scnm = params as *mut EVENT_GROUPS;
			let nm = unsafe {&mut *scnm};
			let handled = me.get_subscription();
			if let Some(needed) = handled {
				*nm = needed;
			}
			handled.is_some()
		},

		EVENT_GROUPS::HANDLE_INITIALIZATION => {
			if params.is_null() { return false as BOOL; }
			let scnm = params as *mut INITIALIZATION_PARAMS;
			let nm = unsafe { &mut *scnm };
			match nm.cmd {
				cmd if cmd == INITIALIZATION_EVENTS::BEHAVIOR_DETACH as UINT => {
					me.detached(he);
				},

				cmd if cmd == INITIALIZATION_EVENTS::BEHAVIOR_ATTACH as UINT => {
					me.attached(he);
				},
				_ => return false as BOOL,
			};
			true
		},

		EVENT_GROUPS::HANDLE_SOM => {
			if params.is_null() { return false as BOOL; }
			let scnm = params as *mut SOM_PARAMS;
			let nm = unsafe { &mut *scnm };
			match nm.cmd {
				cmd if cmd == SOM_EVENTS::SOM_GET_PASSPORT as UINT => {
					if let Some(asset) = me.get_asset() {
						nm.result.passport = asset.get_passport();
						return true as BOOL;
					}
				},

				cmd if cmd == SOM_EVENTS::SOM_GET_ASSET as UINT => {
					if let Some(asset) = me.get_asset() {
						nm.result.asset = asset;
						return true as BOOL;
					}
				},
				_ => return false as BOOL,
			};
			false
		},


		EVENT_GROUPS::HANDLE_BEHAVIOR_EVENT => {
			if params.is_null() { return false as BOOL; }
			let scnm = params as *const BEHAVIOR_EVENT_PARAMS;
			let nm = unsafe { &*scnm };

			let code = BEHAVIOR_EVENTS(nm.cmd & 0x0_0FFF);
			let Some(phase) = decode_phase(nm.cmd & 0xFFFF_F000) else { return false as BOOL; };
			let reason = match code {
				BEHAVIOR_EVENTS::EDIT_VALUE_CHANGED | BEHAVIOR_EVENTS::EDIT_VALUE_CHANGING => {
					edit_event_reason(nm.reason)
				},

				BEHAVIOR_EVENTS::VIDEO_BIND_RQ => {
					EventReason::VideoBind(nm.reason as LPVOID)
				}

				_ => {
					general_event_reason(nm.reason)
				}
			};

			if he.is_null() && code != BEHAVIOR_EVENTS::MEDIA_CHANGED {
				eprintln!("[sciter] warning! null element for {:?}:{:?}", evtg, code);
			}

			if phase == PHASE_MASK::SINKING {	// catch this only once
				match code {
					BEHAVIOR_EVENTS::DOCUMENT_COMPLETE => {
						me.document_complete(he, nm.heTarget);
					},
					BEHAVIOR_EVENTS::DOCUMENT_CLOSE => {
						me.document_close(he, nm.heTarget);
					},
					_ => ()
				};
			}

			let handled = me.on_event(he, nm.he, nm.heTarget, code, phase, reason);
			handled
		},

		EVENT_GROUPS::HANDLE_SCRIPTING_METHOD_CALL => {
			if params.is_null() { return false as BOOL; }
			let scnm = params as *mut SCRIPTING_METHOD_PARAMS;
			let nm = unsafe { &mut *scnm };
			let name = unsafe { u2s!(nm.name) };
			let argv = unsafe { Value::unpack_from(nm.argv, nm.argc) };
			let rv = me.on_script_call(he, &name, &argv);
			let handled = if let Some(v) = rv {
				v.pack_to(&mut nm.result);
				true
			} else {
				false
			};
			handled
		},

    EVENT_GROUPS::HANDLE_METHOD_CALL => {
      if params.is_null() { return false as BOOL; }
      let scnm = params as *const METHOD_PARAMS;
      let nm = unsafe { & *scnm };
      let method = (*nm).method;
      use capi::scbehavior::BEHAVIOR_METHOD_IDENTIFIERS::*;

      // output values
      let mut method_value = Value::new();
      let mut is_empty = false;

      let handled = {

        // unpack method parameters
        use dom::event::MethodParams;
        let reason = match method {
          method if method == DO_CLICK as UINT => {
            MethodParams::Click
          },
          method if method == IS_EMPTY as UINT => {
            MethodParams::IsEmpty(&mut is_empty)
          },
          method if method == GET_VALUE as UINT => {
            MethodParams::GetValue(&mut method_value)
          },
          method if method == SET_VALUE as UINT => {
            // Value from Sciter.
            let payload = params as *const VALUE_PARAMS;
            let pm = unsafe { & *payload };
            MethodParams::SetValue(Value::from(&pm.value))
          },

          _ => {
            MethodParams::Custom((*nm).method, params)
          },
        };

        // call event handler
        let handled = me.on_method_call(he, reason);
        handled
      };

      if handled {
        // Pack values back to Sciter.
        match method {
          method if method == GET_VALUE as UINT => {
            let payload = params as *mut VALUE_PARAMS;
            let pm = unsafe { &mut *payload };
            method_value.pack_to(&mut pm.value);
          },

          method if method == IS_EMPTY as UINT => {
            let payload = params as *mut IS_EMPTY_PARAMS;
            let pm = unsafe { &mut *payload };
            pm.is_empty = is_empty as UINT;
          },

          _ => {},
        }
      }
      // we've done here
      handled
    },

		EVENT_GROUPS::HANDLE_TIMER => {
			if params.is_null() { return false as BOOL; }
			let scnm = params as *const TIMER_PARAMS;
			let nm = unsafe { & *scnm };
			let handled = me.on_timer(he, nm.timerId as u64);
			handled
		},

		EVENT_GROUPS::HANDLE_DRAW => {
			if params.is_null() { return false as BOOL; }
			let scnm = params as *const DRAW_PARAMS;
			let nm = unsafe { & *scnm };
			let Some(layer) = decode_draw_layer(nm.layer) else { return false as BOOL; };
			let handled = me.on_draw(he, nm.gfx, &nm.area, layer);
			handled
		},

		// known notifications:
		EVENT_GROUPS::HANDLE_MOUSE
		| EVENT_GROUPS::HANDLE_KEY
		| EVENT_GROUPS::HANDLE_FOCUS
		| EVENT_GROUPS::HANDLE_SCROLL
		| EVENT_GROUPS::HANDLE_SIZE
		| EVENT_GROUPS::HANDLE_DATA_ARRIVED
		| EVENT_GROUPS::HANDLE_EXCHANGE
		| EVENT_GROUPS::HANDLE_GESTURE => {
			// it's a known event but it's not added yet to dom::EventHandler
			// they can be added if somebody needs it
			false
		}

		// unknown `EVENT_GROUPS` notification
		_ => {
			eprintln!("[sciter] warning! unknown event group {:04X}", evtg.0 as u32);
			false
		},
	};
	return result as BOOL;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn phase_decoder_rejects_unknown_bits() {
		assert_eq!(decode_phase(0), Some(PHASE_MASK::BUBBLING));
		assert_eq!(decode_phase(PHASE_MASK::SINKING as UINT), Some(PHASE_MASK::SINKING));
		assert_eq!(decode_phase(u32::MAX), None);
	}

	#[test]
	fn draw_layer_decoder_rejects_unknown_values() {
		assert_eq!(decode_draw_layer(3), Some(DRAW_EVENTS::DRAW_OUTLINE));
		assert_eq!(decode_draw_layer(u32::MAX), None);
	}

	#[test]
	fn arbitrary_event_reasons_are_preserved_without_enum_casts() {
		assert!(matches!(general_event_reason(usize::MAX), EventReason::Unknown(usize::MAX)));
		assert!(matches!(edit_event_reason(usize::MAX), EventReason::Unknown(usize::MAX)));
	}
}
