use egui::epaint::Primitive;
use egui::{OutputCommand, TextureId};
use sdl3_sys::error::SDL_GetError;
use sdl3_sys::events::{SDL_Event, SDL_EventType};
use sdl3_sys::keyboard::{SDL_GetModState, SDL_StartTextInput, SDL_StopTextInput};
use sdl3_sys::keycode::SDL_Keycode;
use sdl3_sys::mouse::{SDL_CreateSystemCursor, SDL_Cursor, SDL_DestroyCursor, SDL_SystemCursor};
use sdl3_sys::pixels::SDL_FColor;
use sdl3_sys::rect::{SDL_FPoint, SDL_Rect};
use sdl3_sys::render::{
    SDL_CreateTexture, SDL_DestroyTexture, SDL_GetRenderScale, SDL_SetRenderScale, SDL_Texture,
    SDL_UpdateTexture, SDL_Vertex,
};
use sdl3_sys::stdinc::SDL_free;
use sdl3_sys::video::{SDL_GetWindowSize, SDL_GetWindowSizeInPixels, SDL_Window};
use sdl3_sys::{clipboard, keycode, mouse, pixels, render};
use std::collections::HashMap;
use std::ffi::CStr;
use std::ptr;
use std::ptr::addr_of_mut;

struct Cursor {
    ptr: *mut SDL_Cursor,
    looks: SDL_SystemCursor,
}
impl Cursor {
    /* SAFETY: This needs to be called from main thread */
    pub fn new(looks: SDL_SystemCursor) -> Result<Self, &'static CStr> {
        unsafe {
            let ptr = SDL_CreateSystemCursor(looks);
            if ptr.is_null() {
                return Err(CStr::from_ptr(SDL_GetError()));
            }
            Ok(Self { ptr, looks })
        }
    }
}
impl Drop for Cursor {
    /* SAFETY: This needs to be called from main thread */
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                SDL_DestroyCursor(self.ptr);
            }
        }
    }
}

struct DrawInfo {
    textures: egui::TexturesDelta,
    primitives: Vec<egui::ClippedPrimitive>,
}

pub struct Painter {
    ctx: egui::Context,
    cursor: Cursor,
    cursor_pos: egui::Pos2,
    modifiers: egui::Modifiers,
    raw_input: egui::RawInput,
    sdl_textures: HashMap<TextureId, *mut SDL_Texture>,
    draw_info: Option<DrawInfo>,
}

impl Painter {
    /* SAFETY: Painter must be intialized after SDL_Window has been created, otherwise getting
     * window size will fail. */
    pub fn new(window: *mut SDL_Window) -> Self {
        let mut screen_size_x = 0;
        let mut screen_size_y = 0;
        unsafe { SDL_GetWindowSize(window, &mut screen_size_x, &mut screen_size_y) };
        let mut screen_pixels_x = 0;
        let mut screen_pixels_y = 0;
        unsafe { SDL_GetWindowSizeInPixels(window, &mut screen_pixels_x, &mut screen_pixels_y) };
        let pixels_per_point = screen_pixels_x as f32 / screen_pixels_x as f32;

        let looks = mouse::SDL_SYSTEM_CURSOR_DEFAULT;
        let cursor = Cursor::new(looks).expect("Failed to init cursor");

        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(pixels_per_point);

        Self {
            ctx,
            cursor,
            cursor_pos: egui::Pos2 { x: 0.0, y: 0.0 },
            modifiers: egui::Modifiers::default(),
            raw_input: egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(screen_size_x as f32, screen_size_y as f32),
                )),
                ..Default::default()
            },
            sdl_textures: Default::default(),
            draw_info: None,
        }
    }

    pub fn update_time(&mut self, duration: f64) {
        self.raw_input.time = Some(duration);
    }

    /* SAFETY: Unsafe interpretation of C union. Clipboard functions needs to be run from main
     * thread. */
    pub fn handle_event(&mut self, event: SDL_Event, window: *mut SDL_Window) -> bool {
        let mut handled = false;
        let event_type = unsafe { SDL_EventType(event.r#type) };
        match event_type {
            SDL_EventType::WINDOW_RESIZED | SDL_EventType::WINDOW_PIXEL_SIZE_CHANGED => {
                let x = unsafe { event.window.data1 as f32 };
                let y = unsafe { event.window.data2 as f32 };
                self.raw_input.screen_rect = Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2 { x, y },
                ));
            }
            SDL_EventType::MOUSE_BUTTON_DOWN => {
                // Click is set to handled only if it was made inside the area of the widget
                // If egui is expecting keyboard input, clicking outside the area will cancel
                // any input, but not mark the click as handled.
                if self.ctx.is_pointer_over_area()
                    || self.ctx.wants_pointer_input()
                    || self.ctx.wants_keyboard_input()
                {
                    handled = self.ctx.is_pointer_over_area();
                    let btn = unsafe {
                        match event.button.button as i32 {
                            mouse::SDL_BUTTON_LEFT => Some(egui::PointerButton::Primary),
                            mouse::SDL_BUTTON_MIDDLE => Some(egui::PointerButton::Middle),
                            mouse::SDL_BUTTON_RIGHT => Some(egui::PointerButton::Secondary),
                            _ => None,
                        }
                    };
                    if let Some(btn) = btn {
                        self.raw_input.events.push(egui::Event::PointerButton {
                            pos: self.cursor_pos,
                            button: btn,
                            pressed: true,
                            modifiers: self.modifiers,
                        });
                    }
                }
            }
            SDL_EventType::MOUSE_BUTTON_UP => {
                if self.ctx.wants_pointer_input() {
                    let btn = unsafe {
                        match event.button.button as i32 {
                            mouse::SDL_BUTTON_LEFT => Some(egui::PointerButton::Primary),
                            mouse::SDL_BUTTON_MIDDLE => Some(egui::PointerButton::Middle),
                            mouse::SDL_BUTTON_RIGHT => Some(egui::PointerButton::Secondary),
                            _ => None,
                        }
                    };
                    if let Some(btn) = btn {
                        self.raw_input.events.push(egui::Event::PointerButton {
                            pos: self.cursor_pos,
                            button: btn,
                            pressed: false,
                            modifiers: self.modifiers,
                        });
                    }
                    handled = true;
                }
            }
            SDL_EventType::MOUSE_MOTION => {
                let x = unsafe { event.motion.x as f32 };
                let y = unsafe { event.motion.y as f32 };
                let screen_rect = self.ctx.screen_rect();
                self.cursor_pos.x = x.clamp(screen_rect.min.x, screen_rect.max.x - 1.0);
                self.cursor_pos.y = y.clamp(screen_rect.min.y, screen_rect.max.y - 1.0);
                self.raw_input
                    .events
                    .push(egui::Event::PointerMoved(self.cursor_pos));
            }
            SDL_EventType::MOUSE_WHEEL => {
                if self.ctx.wants_pointer_input() {
                    let x = unsafe { event.wheel.x as f32 };
                    let y = unsafe { event.wheel.y as f32 };
                    let delta = egui::Vec2::new(x, y);
                    let mod_state = unsafe { SDL_GetModState() };
                    let left_ctrl = mod_state & keycode::SDL_KMOD_LCTRL > 0;
                    let right_ctrl = mod_state & keycode::SDL_KMOD_RCTRL > 0;

                    if left_ctrl || right_ctrl {
                        self.raw_input
                            .events
                            .push(egui::Event::Zoom((delta.y / 125.0).exp()));
                    }
                    handled = true;
                }
            }
            SDL_EventType::KEY_DOWN => {
                if self.ctx.wants_keyboard_input() {
                    let keycode = unsafe { event.key.key };
                    if keycode != keycode::SDLK_UNKNOWN {
                        if let Some(key) = sdl_key_to_egui(keycode) {
                            self.modifiers = get_modifiers();
                            self.raw_input.modifiers = self.modifiers;

                            if self.modifiers.command {
                                match key {
                                    egui::Key::C => self.raw_input.events.push(egui::Event::Copy),
                                    egui::Key::X => self.raw_input.events.push(egui::Event::Cut),
                                    egui::Key::V => unsafe {
                                        if clipboard::SDL_HasClipboardText() {
                                            let text = clipboard::SDL_GetClipboardText();
                                            if !text.is_null() {
                                                if let Ok(text) = CStr::from_ptr(text).to_str() {
                                                    self.raw_input
                                                        .events
                                                        .push(egui::Event::Text(text.to_string()));
                                                }
                                                SDL_free(text as *mut _);
                                            }
                                        }
                                    },
                                    _ => {}
                                }
                            }

                            unsafe { SDL_StartTextInput(window) };
                            self.raw_input.focused = true;
                            self.raw_input.events.push(egui::Event::Key {
                                key,
                                physical_key: Some(key),
                                pressed: true,
                                repeat: false,
                                modifiers: self.modifiers,
                            });
                            handled = true;
                        }
                    }
                }
            }
            SDL_EventType::KEY_UP => {
                if self.ctx.wants_keyboard_input() {
                    let keycode = unsafe { event.key.key };

                    match keycode {
                        keycode::SDLK_UNKNOWN => {}
                        keycode::SDLK_ESCAPE => unsafe {
                            SDL_StopTextInput(window);
                        },
                        _ => {
                            if let Some(key) = sdl_key_to_egui(keycode) {
                                self.modifiers = get_modifiers();
                                self.raw_input.modifiers = self.modifiers;

                                self.raw_input.events.push(egui::Event::Key {
                                    key,
                                    physical_key: Some(key),
                                    pressed: false,
                                    repeat: false,
                                    modifiers: self.modifiers,
                                });
                                handled = true;
                            }
                        }
                    }
                }
            }
            SDL_EventType::TEXT_INPUT => unsafe {
                if self.ctx.wants_keyboard_input() {
                    self.modifiers = get_modifiers();
                    self.raw_input.modifiers = self.modifiers;
                    let text = event.text.text;
                    let text = CStr::from_ptr(text);
                    if let Ok(text) = text.to_str() {
                        self.raw_input
                            .events
                            .push(egui::Event::Text(text.to_string()));
                        handled = true;
                    }
                }
            },
            _ => {}
        }

        handled
    }

    pub fn begin_pass(&mut self) -> egui::Context {
        self.ctx.begin_pass(self.raw_input.take());
        self.ctx.clone()
    }

    /* SAFETY: This needs to be called from main thread */
    pub fn end_pass(&mut self) {
        let output = self.ctx.end_pass();
        for cmd in output.platform_output.commands {
            match cmd {
                OutputCommand::CopyText(text) => {
                    if let Ok(text) = std::ffi::CString::new(text) {
                        unsafe {
                            if !clipboard::SDL_SetClipboardText(text.as_ptr()) {
                                println!("{:?}", SDL_GetError());
                            };
                        }
                    }
                }
                _ => {}
            }
        }

        if !self.cursor.ptr.is_null() {
            use sdl3_sys::mouse::SDL_SystemCursor;
            let new_cursor_look = match output.platform_output.cursor_icon {
                egui::CursorIcon::Crosshair => SDL_SystemCursor::CROSSHAIR,
                egui::CursorIcon::Default => SDL_SystemCursor::DEFAULT,
                egui::CursorIcon::Grab => SDL_SystemCursor::POINTER,
                egui::CursorIcon::Grabbing => SDL_SystemCursor::MOVE,
                egui::CursorIcon::Move => SDL_SystemCursor::MOVE,
                egui::CursorIcon::PointingHand => SDL_SystemCursor::POINTER,
                egui::CursorIcon::ResizeHorizontal => SDL_SystemCursor::EW_RESIZE,
                egui::CursorIcon::ResizeNeSw => SDL_SystemCursor::NESW_RESIZE,
                egui::CursorIcon::ResizeNwSe => SDL_SystemCursor::NWSE_RESIZE,
                egui::CursorIcon::ResizeVertical => SDL_SystemCursor::NS_RESIZE,
                egui::CursorIcon::Text => SDL_SystemCursor::TEXT,
                egui::CursorIcon::NotAllowed | egui::CursorIcon::NoDrop => {
                    SDL_SystemCursor::NOT_ALLOWED
                }
                egui::CursorIcon::Wait => SDL_SystemCursor::WAIT,
                _ => SDL_SystemCursor::DEFAULT,
            };

            if new_cursor_look != self.cursor.looks {
                unsafe {
                    match Cursor::new(new_cursor_look) {
                        Ok(cursor) => {
                            self.cursor = cursor;
                            mouse::SDL_SetCursor(self.cursor.ptr);
                        }
                        Err(e) => {
                            match e.to_str() {
                                Ok(text) => println!("Failed to set cursor: {}", text),
                                _ => println!("Failed to set cursor"),
                            };
                        }
                    }
                }
            }
        }

        let clipped_primitives = self
            .ctx
            .tessellate(output.shapes.clone(), self.ctx.pixels_per_point());
        self.draw_info = Some(DrawInfo {
            textures: output.textures_delta,
            primitives: clipped_primitives,
        });
    }

    /* SAFETY: This needs to be called from main thread */
    pub fn draw(&mut self, renderer: *mut render::SDL_Renderer) {
        if self.draw_info.is_none() {
            return;
        }

        let DrawInfo {
            textures,
            primitives,
        } = self.draw_info.take().unwrap();

        let mut render_scale_x = 0.0;
        let mut render_scale_y = 0.0;
        unsafe {
            SDL_GetRenderScale(
                renderer,
                addr_of_mut!(render_scale_x),
                addr_of_mut!(render_scale_y),
            );
            SDL_SetRenderScale(renderer, 1.0, 1.0);
        }

        for (id, image_delta) in textures.set {
            match image_delta.image {
                egui::ImageData::Color(ref color_image) => {
                    let texture = self
                        .sdl_textures
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| unsafe {
                            SDL_CreateTexture(
                                renderer,
                                pixels::SDL_PIXELFORMAT_RGBA32,
                                render::SDL_TEXTUREACCESS_STATIC,
                                color_image.width() as i32,
                                color_image.height() as i32,
                            )
                        });

                    let sdl_pixels: Vec<u8> = color_image
                        .pixels
                        .iter()
                        .flat_map(|color| [color.r(), color.g(), color.b(), color.a()])
                        .collect();

                    unsafe {
                        if let Some(rect) = image_delta.pos {
                            let x = rect[0] as i32;
                            let y = rect[1] as i32;
                            let rect = SDL_Rect {
                                x,
                                y,
                                w: color_image.width() as i32 - x,
                                h: color_image.height() as i32 - y,
                            };

                            // Update partial texture
                            SDL_UpdateTexture(
                                texture,
                                ptr::addr_of!(rect),
                                sdl_pixels.as_ptr() as *const std::ffi::c_void,
                                (color_image.width() * 4) as i32,
                            );
                        } else {
                            // Update full texture
                            SDL_UpdateTexture(
                                texture,
                                ptr::null(),
                                sdl_pixels.as_ptr() as *const std::ffi::c_void,
                                (color_image.width() * 4) as i32,
                            );
                        }
                    }

                    self.sdl_textures.insert(id, texture);
                }
            }
        }
        for id in textures.free {
            unsafe {
                if let Some(t) = self.sdl_textures.get(&id).cloned() {
                    SDL_DestroyTexture(t);
                }
            }
            self.sdl_textures.remove(&id);
        }

        for egui::ClippedPrimitive {
            clip_rect,
            primitive,
        } in &primitives
        {
            let clip = SDL_Rect {
                x: clip_rect.min.x as i32,
                y: clip_rect.min.y as i32,
                w: (clip_rect.max.x - clip_rect.min.x) as i32,
                h: (clip_rect.max.y - clip_rect.min.y) as i32,
            };
            unsafe { render::SDL_SetRenderClipRect(renderer, &clip) };

            match primitive {
                Primitive::Mesh(mesh) => {
                    let sdl_vertices: Vec<SDL_Vertex> = mesh
                        .vertices
                        .iter()
                        .map(|v| SDL_Vertex {
                            position: SDL_FPoint {
                                x: v.pos.x,
                                y: v.pos.y,
                            },
                            color: SDL_FColor {
                                r: v.color.r() as f32 / 255.0,
                                g: v.color.g() as f32 / 255.0,
                                b: v.color.b() as f32 / 255.0,
                                a: v.color.a() as f32 / 255.0,
                            },
                            tex_coord: SDL_FPoint {
                                x: v.uv.x,
                                y: v.uv.y,
                            },
                        })
                        .collect();

                    let sdl_indices: Vec<i32> = mesh.indices.iter().map(|&i| i as i32).collect();

                    let t = self
                        .sdl_textures
                        .get(&mesh.texture_id)
                        .cloned()
                        .unwrap_or_else(|| ptr::null_mut());
                    unsafe {
                        render::SDL_RenderGeometry(
                            renderer,
                            t,
                            sdl_vertices.as_ptr(),
                            sdl_vertices.len() as i32,
                            sdl_indices.as_ptr(),
                            sdl_indices.len() as i32,
                        );
                    }
                }
                Primitive::Callback(_) => {
                    unimplemented!()
                }
            }
        }

        unsafe {
            SDL_SetRenderScale(renderer, render_scale_x, render_scale_y);
        }
    }
}

/* SAFETY: Safe to call from any thread. Unsafe due to FFI only. */
fn get_modifiers() -> egui::Modifiers {
    let mod_state = unsafe { SDL_GetModState() };
    let alt = mod_state & (keycode::SDL_KMOD_LALT | keycode::SDL_KMOD_RALT) > 0;
    let shift = mod_state & (keycode::SDL_KMOD_LSHIFT | keycode::SDL_KMOD_RSHIFT) > 0;
    let ctrl = mod_state & (keycode::SDL_KMOD_LCTRL | keycode::SDL_KMOD_RCTRL) > 0;

    egui::Modifiers {
        alt,
        ctrl,
        shift,
        command: ctrl,
        ..Default::default()
    }
}

fn sdl_key_to_egui(key: SDL_Keycode) -> Option<egui::Key> {
    use egui::Key;
    use sdl3_sys::keycode::*;
    Some(match key {
        SDLK_LEFT => Key::ArrowLeft,
        SDLK_UP => Key::ArrowUp,
        SDLK_RIGHT => Key::ArrowRight,
        SDLK_DOWN => Key::ArrowDown,
        SDLK_ESCAPE => Key::Escape,
        SDLK_TAB => Key::Tab,
        SDLK_BACKSPACE => Key::Backspace,
        SDLK_SPACE => Key::Space,
        SDLK_RETURN => Key::Enter,
        SDLK_INSERT => Key::Insert,
        SDLK_HOME => Key::Home,
        SDLK_DELETE => Key::Delete,
        SDLK_END => Key::End,
        SDLK_PAGEDOWN => Key::PageDown,
        SDLK_PAGEUP => Key::PageUp,
        SDLK_KP_0 | SDLK_0 => Key::Num0,
        SDLK_KP_1 | SDLK_1 => Key::Num1,
        SDLK_KP_2 | SDLK_2 => Key::Num2,
        SDLK_KP_3 | SDLK_3 => Key::Num3,
        SDLK_KP_4 | SDLK_4 => Key::Num4,
        SDLK_KP_5 | SDLK_5 => Key::Num5,
        SDLK_KP_6 | SDLK_6 => Key::Num6,
        SDLK_KP_7 | SDLK_7 => Key::Num7,
        SDLK_KP_8 | SDLK_8 => Key::Num8,
        SDLK_KP_9 | SDLK_9 => Key::Num9,
        SDLK_A => Key::A,
        SDLK_B => Key::B,
        SDLK_C => Key::C,
        SDLK_D => Key::D,
        SDLK_E => Key::E,
        SDLK_F => Key::F,
        SDLK_G => Key::G,
        SDLK_H => Key::H,
        SDLK_I => Key::I,
        SDLK_J => Key::J,
        SDLK_K => Key::K,
        SDLK_L => Key::L,
        SDLK_M => Key::M,
        SDLK_N => Key::N,
        SDLK_O => Key::O,
        SDLK_P => Key::P,
        SDLK_Q => Key::Q,
        SDLK_R => Key::R,
        SDLK_S => Key::S,
        SDLK_T => Key::T,
        SDLK_U => Key::U,
        SDLK_V => Key::V,
        SDLK_W => Key::W,
        SDLK_X => Key::X,
        SDLK_Y => Key::Y,
        SDLK_Z => Key::Z,
        _ => {
            return None;
        }
    })
}
