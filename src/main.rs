use cgmath::{Point2, Vector2};
use gfx::{self, *};
use ggez::conf;
use ggez::event;
use ggez::graphics::{self, BlendMode, Canvas, DrawParam, Drawable, Shader};
use ggez::input::keyboard::{self, KeyCode};
use ggez::nalgebra as na;
use ggez::timer;
use ggez::{Context, GameResult};
use rand::{self, thread_rng, Rng};
use std::env;
use std::path;

///Padding between the rackets and the edge of the screen
const PADDING: f32 = 10.0;
///width of the middle line
const MIDDLE_LINE_W: f32 = 1.0;
///Height of a racket
const RACKET_HEIGHT: f32 = 100.0;
///Widthe of a racket
const RACKET_WIDTH: f32 = 10.0;
///Racket height devided by two
const RACKET_HEIGHT_HALF: f32 = RACKET_HEIGHT * 0.5;
///Racket width devided by two
const RACKET_WIDTH_HALF: f32 = RACKET_WIDTH * 0.5;
///Diameter of the ball
const BALL_SIZE: f32 = 10.0;
///Radius of the ball
const BALL_SIZE_HALF: f32 = BALL_SIZE * 0.5;
///speed of the player racket
const PLAYER_SPEED: f32 = 600.0;
///starting speed of the ball
const BALL_SPEED: f32 = 270.0;

/// The color cast things take when not illuminated
const AMBIENT_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// The number of rays to cast to. Increasing this number will result in better
/// quality shadows. If you increase too much you might hit some GPU shader
/// hardware limits.
const LIGHT_RAY_COUNT: u16 = 620;
/// The strength of the light - how far it shines
const LIGHT_STRENGTH: f32 = 0.0005;
/// The factor at which the light glows - just for fun
const LIGHT_GLOW_FACTOR: f32 = 0.000005;
/// The rate at which the glow effect oscillates
const LIGHT_GLOW_RATE: f32 = 5.0;

// I have noe clue what the hell the below code does
// I just yanked it from the examples lol
// https://github.com/ggez/ggez/blob/master/examples/shadows.rs

gfx_defines! {
    /// Constants used by the shaders to calculate stuff
    constant Light {
        light_color: [f32; 4] = "u_LightColor",
        shadow_color: [f32; 4] = "u_ShadowColor",
        pos: [f32; 2] = "u_Pos",
        screen_size: [f32; 2] = "u_ScreenSize",
        glow: f32 = "u_Glow",
        strength: f32 = "u_Strength",
    }
}

const OCCLUSIONS_SHADER_SOURCE: &[u8] = b"#version 150 core
uniform sampler2D t_Texture;
in vec2 v_Uv;
out vec4 Target0;
layout (std140) uniform Light {
    vec4 u_LightColor;
    vec4 u_ShadowColor;
    vec2 u_Pos;
    vec2 u_ScreenSize;
    float u_Glow;
    float u_Strength;
};
void main() {
    float dist = 1.0;
    float theta = radians(v_Uv.x * 360.0);
    vec2 dir = vec2(cos(theta), sin(theta));
    for(int i = 0; i < 1024; i++) {
        float fi = i;
        float r = fi / 1024.0;
        vec2 rel = r * dir;
        vec2 p = clamp(u_Pos+rel, 0.0, 1.0);
        if (texture(t_Texture, p).a > 0.8) {
            dist = distance(u_Pos, p) * 0.5;
            break;
        }
    }
    float others = dist == 1.0 ? 0.0 : dist;
    Target0 = vec4(dist, others, others, 1.0);
}
";

const VERTEX_SHADER_SOURCE: &[u8] = include_bytes!("../resources/basic_150.glslv");

/// Shader for drawing shadows based on a 1D shadow map. It takes current
/// fragment coordinates and converts them to polar coordinates centered
/// around the light source, using the angle to sample from the 1D shadow map.
/// If the distance from the light source is greater than the distance of the
/// closest reported shadow, then the output is the shadow color, else it calculates some
/// shadow based on the distance from light source based on strength and glow
/// uniform parameters.
const SHADOWS_SHADER_SOURCE: &[u8] = b"#version 150 core
uniform sampler2D t_Texture;
in vec2 v_Uv;
out vec4 Target0;
layout (std140) uniform Light {
    vec4 u_LightColor;
    vec4 u_ShadowColor;
    vec2 u_Pos;
    vec2 u_ScreenSize;
    float u_Glow;
    float u_Strength;
};
void main() {
    vec2 coord = gl_FragCoord.xy / u_ScreenSize;
    vec2 rel = coord - u_Pos;
    float theta = atan(rel.y, rel.x);
    float ox = degrees(theta) / 360.0;
    if (ox < 0) {
        ox += 1.0;
    }
    float r = length(rel);
    float occl = texture(t_Texture, vec2(ox, 0.5)).r * 2.0;
    float intensity = 1.0;
    if (r < occl) {
        vec2 g = u_ScreenSize / u_ScreenSize.y;
        float p = u_Strength + u_Glow;
        float d = distance(g * coord, g * u_Pos);
        intensity = 1.0 - clamp(p/(d*d), 0.0, 1.0);
    }
    Target0 = mix(vec4(1.0, 1.0, 1.0, 1.0), vec4(u_ShadowColor.rgb, 1.0), intensity);
}
";

/// Shader for drawing lights based on a 1D shadow map. It takes current
/// fragment coordinates and converts them to polar coordinates centered
/// around the light source, using the angle to sample from the 1D shadow map.
/// If the distance from the light source is greater than the distance of the
/// closest reported shadow, then the output is black, else it calculates some
/// light based on the distance from light source based on strength and glow
/// uniform parameters. It is meant to be used additively for drawing multiple
/// lights.
const LIGHTS_SHADER_SOURCE: &[u8] = b"#version 150 core
uniform sampler2D t_Texture;
in vec2 v_Uv;
out vec4 Target0;
layout (std140) uniform Light {
    vec4 u_LightColor;
    vec4 u_ShadowColor;
    vec2 u_Pos;
    vec2 u_ScreenSize;
    float u_Glow;
    float u_Strength;
};
void main() {
    vec2 coord = gl_FragCoord.xy / u_ScreenSize;
    vec2 rel = coord - u_Pos;
    float theta = atan(rel.y, rel.x);
    float ox = degrees(theta) / 360.0;
    if (ox < 0) {
        ox += 1.0;
    }
    float r = length(rel);
    float occl = texture(t_Texture, vec2(ox, 0.5)).r * 2.0;
    float intensity = 0.0;
    if (r < occl) {
        vec2 g = u_ScreenSize / u_ScreenSize.y;
        float p = u_Strength + u_Glow;
        float d = distance(g * coord, g * u_Pos);
        intensity = clamp(p/(d*d), 0.0, 0.6);
    }
    Target0 = mix(vec4(0.0, 0.0, 0.0, 1.0), vec4(u_LightColor.rgb, 1.0), intensity);
}
";

// Now we are mostly done with the scary code above

/// Clamps a value between a set amount of values
fn clamp(value: &mut f32, low: f32, high: f32) {
    if *value < low {
        *value = low;
    } else if *value > high {
        *value = high;
    }
}

/// Moves the racket up and down
fn move_racket(pos: &mut na::Point2<f32>, keycode: KeyCode, y_dir: f32, ctx: &mut Context) {
    let screen_h = graphics::drawable_size(ctx).1;
    let dt = timer::delta(ctx).as_secs_f32();

    if keyboard::is_key_pressed(ctx, keycode) {
        pos.y -= PLAYER_SPEED * dt * y_dir;
    }
    clamp(
        &mut pos.y,
        RACKET_HEIGHT_HALF,
        screen_h - RACKET_HEIGHT_HALF,
    );
}

/// Randomizes the starting orientation
fn randomize_vec(vec: &mut na::Vector2<f32>, x: f32, y: f32) {
    let mut rng = thread_rng();
    vec.x = match rng.gen_bool(0.5) {
        true => x,
        false => -x,
    };
    vec.y = match rng.gen_bool(0.5) {
        true => y,
        false => -y,
    };
}

struct MainState {
    player_1_pos: na::Point2<f32>,
    player_2_pos: na::Point2<f32>,
    racket_mesh: graphics::Mesh,
    racket_mesh_2: graphics::Mesh,
    ball_pos: na::Point2<f32>,
    ball_vel: na::Vector2<f32>,
    ball_mesh: graphics::Mesh,
    middle_mesh: graphics::Mesh,
    player_1_score: i32,
    player_2_score: i32,
    background: graphics::Image,
    torch: Light,
    foreground: Canvas,
    occlusions: Canvas,
    shadows: Canvas,
    lights: Canvas,
    occlusions_shader: Shader<Light>,
    shadows_shader: Shader<Light>,
    lights_shader: Shader<Light>,
}

impl MainState {
    pub fn new(ctx: &mut Context) -> GameResult<MainState> {
        let (screen_w, screen_h) = graphics::drawable_size(ctx);
        let (screen_w_half, screen_h_half) = (screen_w * 0.5, screen_h * 0.5);

        let mut ball_vel = na::Vector2::new(0.0, 0.0);
        randomize_vec(&mut ball_vel, BALL_SPEED, BALL_SPEED);

        let racket_rect = graphics::Rect::new(
            -RACKET_WIDTH_HALF,
            -RACKET_HEIGHT_HALF,
            RACKET_WIDTH,
            RACKET_HEIGHT,
        );

        let racket_mesh = graphics::Mesh::new_rectangle(
            ctx,
            graphics::DrawMode::fill(),
            racket_rect,
            graphics::Color::new(0.0, 0.0, 1.0, 1.0),
        )?;

        let racket_mesh_2 = graphics::Mesh::new_rectangle(
            ctx,
            graphics::DrawMode::fill(),
            racket_rect,
            graphics::Color::new(1.0, 0.0, 0.0, 1.0),
        )?;

        let ball_mesh = graphics::Mesh::new_circle(
            ctx,
            graphics::DrawMode::fill(),
            Point2::new(-BALL_SIZE_HALF, -BALL_SIZE_HALF),
            BALL_SIZE,
            0.1,
            graphics::Color::new(1.0, 0.0, 1.0, 1.0),
        )?;

        let middle_rect = graphics::Rect::new(-MIDDLE_LINE_W * 0.5, 0.0, MIDDLE_LINE_W, screen_h);
        let middle_mesh = graphics::Mesh::new_rectangle(
            ctx,
            graphics::DrawMode::fill(),
            middle_rect,
            graphics::WHITE,
        )?;

        let screen_size = {
            let size = graphics::drawable_size(ctx);
            [size.0 as f32, size.1 as f32]
        };
        //add a background image
        let background = graphics::Image::new(ctx, "/bg_top.png")?;
        //set the light
        let torch = Light {
            pos: [screen_w_half, screen_h_half],
            light_color: [1.0, 0.0, 1.0, 1.0],
            shadow_color: AMBIENT_COLOR,
            screen_size,
            glow: 0.0,
            strength: LIGHT_STRENGTH,
        };
        let foreground = Canvas::with_window_size(ctx)?;
        let occlusions = Canvas::new(ctx, LIGHT_RAY_COUNT, 1, conf::NumSamples::One)?;
        let mut shadows = Canvas::with_window_size(ctx)?;
        // The shadow map will be drawn on top using the multiply blend mode
        shadows.set_blend_mode(Some(BlendMode::Multiply));
        let mut lights = Canvas::with_window_size(ctx)?;
        // The light map will be drawn on top using the add blend mode
        lights.set_blend_mode(Some(BlendMode::Add));

        let occlusions_shader = Shader::from_u8(
            ctx,
            VERTEX_SHADER_SOURCE,
            OCCLUSIONS_SHADER_SOURCE,
            torch,
            "Light",
            None,
        )
        .unwrap();
        let shadows_shader = Shader::from_u8(
            ctx,
            VERTEX_SHADER_SOURCE,
            SHADOWS_SHADER_SOURCE,
            torch,
            "Light",
            None,
        )
        .unwrap();
        let lights_shader = Shader::from_u8(
            ctx,
            VERTEX_SHADER_SOURCE,
            LIGHTS_SHADER_SOURCE,
            torch,
            "Light",
            Some(&[BlendMode::Add]),
        )
        .unwrap();

        Ok(MainState {
            player_1_pos: na::Point2::new(RACKET_WIDTH_HALF + PADDING, screen_h_half),
            player_2_pos: na::Point2::new(screen_w - RACKET_WIDTH_HALF - PADDING, screen_h_half),
            racket_mesh,
            racket_mesh_2,
            ball_pos: na::Point2::new(screen_w_half, screen_h_half),
            ball_vel,
            ball_mesh,
            middle_mesh,
            player_1_score: 0,
            player_2_score: 0,
            background,
            torch,
            foreground,
            occlusions,
            shadows,
            lights,
            occlusions_shader,
            shadows_shader,
            lights_shader,
        })
    }

    //se example and official documentation
    fn render_light(
        &mut self,
        ctx: &mut Context,
        light: Light,
        origin: DrawParam,
        canvas_origin: DrawParam,
    ) -> GameResult {
        let size = graphics::size(ctx);
        // Now we want to run the occlusions shader to calculate our 1D shadow
        // distances into the `occlusions` canvas.
        graphics::set_canvas(ctx, Some(&self.occlusions));
        {
            let _shader_lock = graphics::use_shader(ctx, &self.occlusions_shader);

            self.occlusions_shader.send(ctx, light)?;
            graphics::draw(ctx, &self.foreground, canvas_origin)?;
        }

        // Now we render our shadow map and light map into their respective
        // canvases based on the occlusion map. These will then be drawn onto
        // the final render target using appropriate blending modes.
        graphics::set_canvas(ctx, Some(&self.shadows));
        {
            let _shader_lock = graphics::use_shader(ctx, &self.shadows_shader);

            let param = origin.scale(Vector2::new(
                (size.0 as f32) / (LIGHT_RAY_COUNT as f32),
                size.1 as f32,
            ));
            self.shadows_shader.send(ctx, light)?;
            graphics::draw(ctx, &self.occlusions, param)?;
        }
        graphics::set_canvas(ctx, Some(&self.lights));
        {
            let _shader_lock = graphics::use_shader(ctx, &self.lights_shader);

            let param = origin.scale(Vector2::new(
                (size.0 as f32) / (LIGHT_RAY_COUNT as f32),
                size.1 as f32,
            ));
            self.lights_shader.send(ctx, light)?;
            graphics::draw(ctx, &self.occlusions, param)?;
        }
        Ok(())
    }
}

impl event::EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let dt = timer::delta(ctx).as_secs_f32();
        let (screen_w, screen_h) = graphics::drawable_size(ctx);

        move_racket(&mut self.player_1_pos, KeyCode::W, 1.0, ctx);
        move_racket(&mut self.player_1_pos, KeyCode::S, -1.0, ctx);
        move_racket(&mut self.player_2_pos, KeyCode::Up, 1.0, ctx);
        move_racket(&mut self.player_2_pos, KeyCode::Down, -1.0, ctx);

        self.ball_pos += self.ball_vel * dt;

        if self.ball_pos.x < 0.0 {
            self.ball_pos.x = screen_w * 0.5;
            self.ball_pos.y = screen_h * 0.5;
            randomize_vec(&mut self.ball_vel, BALL_SPEED, BALL_SPEED);
            self.player_2_score += 1;
        }
        if self.ball_pos.x > screen_w {
            self.ball_pos.x = screen_w * 0.5;
            self.ball_pos.y = screen_h * 0.5;
            randomize_vec(&mut self.ball_vel, BALL_SPEED, BALL_SPEED);
            self.player_1_score += 1;
        }
        if self.ball_pos.y < BALL_SIZE_HALF {
            self.ball_pos.y = BALL_SIZE_HALF;
            self.ball_vel.y = self.ball_vel.y.abs();
        } else if self.ball_pos.y > screen_h - BALL_SIZE_HALF {
            self.ball_pos.y = screen_h - BALL_SIZE_HALF;
            self.ball_vel.y = -self.ball_vel.y.abs();
        }

        //uncomment the following lines to disable "AI"
        if self.ball_pos.y < self.player_2_pos.y {
            self.player_2_pos.y -= PLAYER_SPEED * 0.4 * dt;
        } else {
            self.player_2_pos.y += PLAYER_SPEED * 0.4 * dt;
        }
        clamp(
            &mut self.player_2_pos.y,
            RACKET_HEIGHT_HALF,
            screen_h - RACKET_HEIGHT_HALF,
        );

        let intersects_player_1 = self.ball_pos.x - BALL_SIZE_HALF
            < self.player_1_pos.x + RACKET_WIDTH_HALF
            && self.ball_pos.x + BALL_SIZE_HALF > self.player_1_pos.x - RACKET_WIDTH_HALF
            && self.ball_pos.y - BALL_SIZE_HALF < self.player_1_pos.y + RACKET_HEIGHT_HALF
            && self.ball_pos.y + BALL_SIZE_HALF > self.player_1_pos.y - RACKET_HEIGHT_HALF;

        if intersects_player_1 {
            self.ball_pos.x = RACKET_WIDTH * 2.0 + PADDING;
            self.ball_vel.x = self.ball_vel.x.abs() + 30.0;
            //change color of ball
            self.torch.light_color = [0.0, 0.0, 1.0, 1.0];
            self.ball_mesh = graphics::Mesh::new_circle(
                ctx,
                graphics::DrawMode::fill(),
                Point2::new(-BALL_SIZE_HALF, -BALL_SIZE_HALF),
                BALL_SIZE,
                0.1,
                graphics::Color::new(0.0, 0.0, 1.0, 1.0),
            )?;
        }
        let intersects_player_2 = self.ball_pos.x - BALL_SIZE_HALF
            < self.player_2_pos.x + RACKET_WIDTH_HALF
            && self.ball_pos.x + BALL_SIZE_HALF > self.player_2_pos.x - RACKET_WIDTH_HALF
            && self.ball_pos.y - BALL_SIZE_HALF < self.player_2_pos.y + RACKET_HEIGHT_HALF
            && self.ball_pos.y + BALL_SIZE_HALF > self.player_2_pos.y - RACKET_HEIGHT_HALF;

        if intersects_player_2 {
            self.ball_pos.x = screen_w - RACKET_WIDTH * 2.0 - PADDING;
            self.ball_vel.x = -self.ball_vel.x.abs() - 30.0;
            //change color of ball
            self.torch.light_color = [1.0, 0.0, 0.0, 1.0];
            self.ball_mesh = graphics::Mesh::new_circle(
                ctx,
                graphics::DrawMode::fill(),
                Point2::new(-BALL_SIZE_HALF, -BALL_SIZE_HALF),
                BALL_SIZE,
                0.1,
                graphics::Color::new(1.0, 0.0, 0.0, 1.0),
            )?;
        }

        self.torch.glow = LIGHT_GLOW_FACTOR * ((timer::ticks(ctx) as f32) / LIGHT_GLOW_RATE).cos();

        // change the light to follow the ball
        // It took quite a bit of thinking to get this sorted
        // the light has a f32 value between 0 and 1
        // AND it's origin is the lower left corner instead of the
        // upper right
        let torch_x = (self.ball_pos.x - BALL_SIZE_HALF) / screen_w;
        let torch_y = 0.5 + 0.5 - ((self.ball_pos.y - BALL_SIZE_HALF) / screen_h);
        self.torch.pos = [torch_x, torch_y];

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let screen_w = graphics::drawable_size(ctx).0;

        let origin = DrawParam::new()
            .dest(Point2::new(0.0, 0.0))
            .scale(Vector2::new(0.5, 0.5));
        let canvas_origin = DrawParam::new();

        // First thing we want to do it to render all the foreground items (that
        // will have shadows) onto their own Canvas (off-screen render). We will
        // use this canvas to:
        //  - run the occlusions shader to determine where the shadows are
        //  - render to screen once all the shadows are calculated and rendered
        {
            graphics::set_canvas(ctx, Some(&self.foreground));
            graphics::clear(ctx, graphics::Color::new(0.0, 0.0, 0.0, 0.0));

            graphics::draw(
                ctx,
                &self.racket_mesh,
                DrawParam::new().dest(Point2::new(self.player_1_pos.x, self.player_1_pos.y)),
            )?;

            graphics::draw(
                ctx,
                &self.racket_mesh_2,
                DrawParam::new().dest(Point2::new(self.player_2_pos.x, self.player_2_pos.y)),
            )?;

            let score_text = graphics::Text::new(format!(
                "{}        {}",
                self.player_1_score, self.player_2_score
            ));

            let mut score_pos = na::Point2::new(screen_w * 0.5, 20.0);
            let (score_text_w, score_text_h) = score_text.dimensions(ctx);
            score_pos -= na::Vector2::new(score_text_w as f32 * 0.5, score_text_h as f32 * 0.5);

            graphics::draw(
                ctx,
                &score_text,
                DrawParam::new().dest(Point2::new(score_pos.x, score_pos.y)),
            )?;
        }

        // Then we draw our light and shadow maps
        {
            let torch = self.torch;

            graphics::set_canvas(ctx, Some(&self.lights));
            graphics::clear(ctx, graphics::Color::new(0.0, 0.0, 0.0, 1.0));

            graphics::set_canvas(ctx, Some(&self.shadows));
            graphics::clear(ctx, graphics::Color::new(0.0, 0.0, 0.0, 1.0));
            self.render_light(ctx, torch, origin, canvas_origin)?;
        }

        // Now lets finally render to screen starting with out background, then
        // the shadows and lights overtop and finally our foreground.
        graphics::set_canvas(ctx, None);
        graphics::clear(ctx, graphics::WHITE);
        graphics::draw(ctx, &self.background, DrawParam::default())?;
        graphics::draw(ctx, &self.shadows, DrawParam::default())?;
        graphics::draw(ctx, &self.foreground, DrawParam::default())?;
        graphics::draw(ctx, &self.lights, DrawParam::default())?;

        //we dont want the middle line or the ball to be counted as objects for the light
        // so we render them last
        let screen_middle_x = graphics::drawable_size(ctx).0 * 0.5;
        graphics::draw(
            ctx,
            &self.middle_mesh,
            DrawParam::new().dest(Point2::new(screen_middle_x, 0.0)),
        )?;
        graphics::draw(
            ctx,
            &self.ball_mesh,
            DrawParam::new().dest(Point2::new(self.ball_pos.x, self.ball_pos.y)),
        )?;
        graphics::present(ctx)?;
        Ok(())
    }
}

fn main() -> GameResult {
    let resource_dir = if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        let mut path = path::PathBuf::from(manifest_dir);
        path.push("resources");
        path
    } else {
        path::PathBuf::from("./resources")
    };

    let cb = ggez::ContextBuilder::new("shadows_test", "halvard").add_resource_path(resource_dir);
    let (ctx, event_loop) = &mut cb.build()?;

    let state = &mut MainState::new(ctx)?;
    event::run(ctx, event_loop, state)
}
