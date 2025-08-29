use std::{
    ffi::{CStr, CString},
    ptr::{addr_of_mut, null_mut},
};

use sdl3_sys::{
    error::SDL_GetError,
    events::{SDL_Event, SDL_EventType, SDL_PollEvent},
    init::{SDL_INIT_VIDEO, SDL_Init},
    keycode::SDLK_ESCAPE,
    pixels::SDL_ALPHA_OPAQUE,
    render::{
        SDL_CreateWindowAndRenderer, SDL_RenderClear, SDL_RenderPresent, SDL_Renderer,
        SDL_SetRenderDrawColor,
    },
    timer::SDL_GetTicks,
    video::SDL_Window,
};

pub fn main() -> Result<(), &'static CStr> {
    let title = CString::new("Hell world!").unwrap();
    let mut renderer: *mut SDL_Renderer = null_mut();
    let mut window: *mut SDL_Window = null_mut();
    let mut editor_text = String::new();
    let mut color_picker = [0.0, 0.0, 0.0, 1.0];

    // All calls to SDL are unsafe
    unsafe {
        if !SDL_Init(SDL_INIT_VIDEO) {
            return Err(CStr::from_ptr(SDL_GetError()));
        }

        if !SDL_CreateWindowAndRenderer(
            title.as_ptr(),
            640,
            480,
            0,
            addr_of_mut!(window),
            addr_of_mut!(renderer),
        ) {
            return Err(CStr::from_ptr(SDL_GetError()));
        }
    }

    let mut painter = egui_sdl3::Painter::new(window);

    'main_loop: loop {
        // UPDATE
        let ticks = unsafe { SDL_GetTicks() };
        painter.update_time(ticks as f64 / 1000.0);
        let ctx = painter.begin_pass();
        egui::Window::new("Hello, world!").show(&ctx, |ui| {
            ui.label("Hello, world!");
            if ui.button("Greet").clicked() {
                println!("Hello, world!");
            }
            ui.horizontal(|ui| {
                ui.label("Color: ");
                ui.color_edit_button_rgba_premultiplied(&mut color_picker);
            });
            ui.code_editor(&mut editor_text);
        });
        painter.end_pass();

        // INPUT
        unsafe {
            let mut input_event = SDL_Event::default();
            while SDL_PollEvent(std::ptr::addr_of_mut!(input_event)) {
                if painter.handle_event(input_event, window) {
                    continue;
                }
                let event_type = SDL_EventType(input_event.r#type);
                match event_type {
                    SDL_EventType::TERMINATING | SDL_EventType::QUIT => {
                        break 'main_loop;
                    }
                    SDL_EventType::KEY_DOWN => match input_event.key.key {
                        SDLK_ESCAPE => break 'main_loop,
                        _ => {}
                    },

                    _ => {}
                }
            }
        }

        // RENDER
        unsafe {
            SDL_SetRenderDrawColor(renderer, 245, 245, 245, SDL_ALPHA_OPAQUE);
            SDL_RenderClear(renderer);
        }

        painter.draw(renderer);

        unsafe {
            SDL_RenderPresent(renderer);
        }
    }

    Ok(())
}
