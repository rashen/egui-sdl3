# egui for SDL3

`egui-sdl3` is an [sdl3-backend](https://wiki.libsdl.org/SDL3/FrontPage) for [egui](https://github.com/emilk/egui). This crate uses unsafe C-bindings from [sdl3-sys](https://codeberg.org/maia/sdl3-sys-rs), if you prefer safe bindings I recommend rewriting this to use [sdl3-rs](https://github.com/vhspace/sdl3-rs) instead.

** Please note that this crate has not been released yet, it will most likely contain bugs. **

## Usage

1. Initialize by creating a new Painter object. Note that this must happen after `SDL_Window` has been created.
2. On each loop:
3. Update time with `Painter::update_time()`.
4. Pass input events to `Painter::handle_event()`. This will return true if the event has been consumed.
5. Call `Painter::begin_pass()` to get a `egui` context and start creating your window.
6. Call `Painter::end_pass()` to give back the context
7. Call `Painter::draw()` as part of your render code, make sure ordering is correct so it ends up on top.

