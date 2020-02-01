use std::f64;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, HtmlCanvasElement, CanvasRenderingContext2d};
use alchemy_core::*;

#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

macro_rules! log {
  ( $( $t:tt )* ) => {
    web_sys::console::log_1(&format!( $( $t )* ).into());
  }
}

#[wasm_bindgen]
extern {
  fn alert(s: &str);
}

#[wasm_bindgen]
#[allow(non_snake_case)]
pub fn getAlchemy() -> AlchemyProxy {
  AlchemyProxy::new()
}

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
  #[cfg(debug_assertions)]
  console_error_panic_hook::set_once();

  let document = web_sys::window().unwrap().document().unwrap();
  let _body = document.body().expect("Document should have a body");

  // let val = document.create_element("p")?;
  // val.set_inner_html("Hello from Rust!");
  // body.append_child(&val)?;
  // log!("Hello from macro");

  // setup_canvas(&document);

  Ok(())
}

#[wasm_bindgen]
pub struct AlchemyProxy {
  alchemy: Alchemy
}

#[wasm_bindgen]
impl AlchemyProxy {
  pub fn new() -> Self {
    let mut alchemy = Alchemy::new();
    alchemy.make_scene();
    Self {
      alchemy: alchemy
    }
  }

  pub fn render(&mut self, canvas: HtmlCanvasElement) {
    let w = canvas.width();
    let h = canvas.height();
    let ctx = CanvasContext::new(canvas);
    self.alchemy.render(&ctx, w, h);
  }
}

pub struct CanvasContext {
  canvas: HtmlCanvasElement,
  context: CanvasRenderingContext2d
}

impl CanvasContext {
  pub fn new(canvas: HtmlCanvasElement) -> Self {
    let context = canvas
    .get_context("2d")
    .unwrap()
    .unwrap()
    .dyn_into::<CanvasRenderingContext2d>()
    .unwrap();
    context.scale(canvas.width() as f64, canvas.height() as f64);
    context.translate(0.5, 0.5 * canvas.height() as f64 / canvas.width() as f64);
    Self {
      canvas: canvas,
      context: context
    }
  }
}

impl DrawingContext for CanvasContext {
  fn move_to(&self, x: f64, y: f64) {
    self.context.move_to(x, y);
  }

  fn line_to(&self, x: f64, y: f64) {
    self.context.line_to(x, y);
  }

  fn begin_path(&self) {
    self.context.begin_path();
  }

  fn stroke(&self) {
    self.context.stroke();
  }

  fn clear(&self) {
    self.context.clear_rect(0.0, 0.0, (self.canvas.width()).into(), self.canvas.height().into());
    self.set_line_width(0.02);
    self.context.begin_path();
    self.context.rect(0.0, 0.0, (self.canvas.width()).into(), self.canvas.height().into());
    self.context.stroke();
  }

  fn set_line_width(&self, width: f64) {
    self.context.set_line_width(width);
  }

  fn arc(&self, x: f64, y: f64, radius: f64, start_angle: f64, end_angle: f64) {
    self.context.arc(x, y, radius, start_angle, end_angle);
  }

  fn fill(&self) {
    self.context.fill();
  }

  fn scale(&self, x: f64, y: f64) {
    self.context.scale(x, y);
  }

  fn translate(&self, x: f64, y: f64) {
    self.context.translate(x, y);
  }

  fn set_source_rgb(&self, r: f64, g: f64, b: f64) {
    self.context.set_stroke_style(&JsValue::from_str(
      &format!("rgb({},{},{})", r * 255., g * 255., b * 255.).to_string()
    ));
    self.context.set_fill_style(&JsValue::from_str(
      &format!("rgb({},{},{})", r * 255., g * 255., b * 255.).to_string()
    ));
  }

  fn paint(&self) {
    // self.context.paint();
  }
}

fn setup_canvas(document: &Document) {
  let canvas = document.get_element_by_id("main-canvas").unwrap();
  let canvas: web_sys::HtmlCanvasElement = canvas
    .dyn_into::<HtmlCanvasElement>()
    .map_err(|_| ())
    .unwrap();

  let context = canvas
    .get_context("2d")
    .unwrap()
    .unwrap()
    .dyn_into::<CanvasRenderingContext2d>()
    .unwrap();

  context.set_stroke_style(&JsValue::from_str("rgb(128, 128, 128)"));
  context.set_line_width(2.0);
  context.begin_path();

  // Draw the outer circle.
  context
    .arc(75.0, 75.0, 50.0, 0.0, f64::consts::PI * 2.0)
    .unwrap();

  // Draw the mouth.
  context.move_to(110.0, 75.0);
  context.arc(75.0, 75.0, 35.0, 0.0, f64::consts::PI).unwrap();

  // Draw the left eye.
  context.move_to(65.0, 65.0);
  context
    .arc(60.0, 65.0, 5.0, 0.0, f64::consts::PI * 2.0)
    .unwrap();

  // Draw the right eye.
  context.move_to(95.0, 65.0);
  context
    .arc(90.0, 65.0, 5.0, 0.0, f64::consts::PI * 2.0)
    .unwrap();

  context.stroke();
}
