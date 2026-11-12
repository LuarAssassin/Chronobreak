use crate::{
    render::{DrawOutcome, Renderer, Result},
    ui::{Element, Scene},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

const RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Default)]
struct RedrawSchedule {
    retry_at: Option<Instant>,
}

impl RedrawSchedule {
    fn ready(&self, now: Instant) -> bool {
        self.retry_at.is_none_or(|deadline| now >= deadline)
    }

    fn complete(&mut self, outcome: DrawOutcome, now: Instant) {
        self.retry_at = match outcome {
            DrawOutcome::Presented => None,
            DrawOutcome::Retry => Some(now + RETRY_DELAY),
        };
    }

    fn take_retry(&mut self, now: Instant) -> bool {
        if self.retry_at.is_some() && self.ready(now) {
            self.retry_at = None;
            true
        } else {
            false
        }
    }

    fn control_flow(&self) -> ControlFlow {
        self.retry_at
            .map_or(ControlFlow::Wait, ControlFlow::WaitUntil)
    }
}

pub struct Application {
    renderer: Option<Renderer>,
    window: Option<Arc<Window>>,
    scene: Scene,
    title: String,
    cursor: Option<PhysicalPosition<f64>>,
    occluded: bool,
    error: Option<Box<dyn std::error::Error>>,
    trace: bool,
    redraw_schedule: RedrawSchedule,
}

impl Application {
    pub fn new(root: Element) -> Self {
        Self {
            renderer: None,
            window: None,
            scene: Scene::new(root),
            title: "Chronobreak".into(),
            cursor: None,
            occluded: false,
            error: None,
            trace: std::env::var_os("CHRONOBREAK_TRACE").is_some(),
            redraw_schedule: RedrawSchedule::default(),
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn run(mut self) -> Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::Wait);
        event_loop.run_app(&mut self)?;
        self.error.map_or(Ok(()), Err)
    }

    fn redraw(&mut self) -> Result<()> {
        let window = self.window.as_ref().unwrap();
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 || self.occluded {
            self.redraw_schedule = RedrawSchedule::default();
            return Ok(());
        }
        if !self.redraw_schedule.ready(Instant::now()) {
            return Ok(());
        }
        let scale = window.scale_factor() as f32;
        let changed = self.scene.sync();
        self.scene
            .layout(size.width as f32 / scale, size.height as f32 / scale);
        let outcome = self
            .renderer
            .as_mut()
            .unwrap()
            .draw(&mut self.scene, scale)?;
        self.redraw_schedule.complete(outcome, Instant::now());
        if self.trace && outcome == DrawOutcome::Presented {
            let lines: usize = self
                .scene
                .nodes
                .iter()
                .filter_map(|node| node.buffer.as_ref())
                .map(|buffer| buffer.layout_runs().count())
                .sum();
            eprintln!(
                "frame physical={}x{} scale={scale} changed={changed} text_lines={lines}",
                size.width, size.height,
            );
        }
        Ok(())
    }
}

impl ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let result = (|| -> Result<_> {
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_title(&self.title)
                        .with_inner_size(LogicalSize::new(760.0, 520.0)),
                )?,
            );
            let renderer = pollster::block_on(Renderer::new(window.clone()))?;
            Ok((window, renderer))
        })();
        match result {
            Ok((window, renderer)) => {
                window.request_redraw();
                self.renderer = Some(renderer);
                self.window = Some(window);
            }
            Err(error) => {
                self.error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.renderer = None;
        self.window = None;
        self.cursor = None;
        self.scene.cancel_pointer();
        self.redraw_schedule = RedrawSchedule::default();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window
            && self.redraw_schedule.take_retry(Instant::now())
        {
            window.request_redraw();
        }
        event_loop.set_control_flow(self.redraw_schedule.control_flow());
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = &self.window else {
            return;
        };
        if window.id() != window_id {
            return;
        }
        if self.trace
            && matches!(
                event,
                WindowEvent::CursorMoved { .. }
                    | WindowEvent::CursorLeft { .. }
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::Focused(_)
            )
        {
            eprintln!("input {event:?}");
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.scene.cancel_pointer();
                self.renderer
                    .as_mut()
                    .unwrap()
                    .resize(size.width, size.height);
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.inner_size();
                self.scene.cancel_pointer();
                self.renderer
                    .as_mut()
                    .unwrap()
                    .resize(size.width, size.height);
                window.request_redraw();
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.cursor = Some(position),
            WindowEvent::CursorLeft { .. } | WindowEvent::Focused(false) => {
                self.cursor = None;
                self.scene.cancel_pointer();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(position) = self.cursor {
                    let scale = window.scale_factor();
                    if self.scene.pointer_button(
                        state == ElementState::Pressed,
                        (position.x / scale) as f32,
                        (position.y / scale) as f32,
                    ) {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw() {
                    self.error = Some(error);
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::Signal;

    #[test]
    fn synced_update_is_retried_without_another_input() {
        let counter = Signal::new(0);
        let mut scene = Scene::new(Element::bound_text(&counter, |value| value.to_string()));
        let mut schedule = RedrawSchedule::default();
        let now = Instant::now();
        counter.update(|value| *value += 1);
        assert_eq!(scene.sync(), 1);
        schedule.complete(DrawOutcome::Retry, now);
        assert_eq!(
            schedule.control_flow(),
            ControlFlow::WaitUntil(now + RETRY_DELAY)
        );
        assert!(!schedule.take_retry(now));
        assert!(schedule.take_retry(now + RETRY_DELAY));
        assert_eq!(scene.sync(), 0);
        assert!(
            scene
                .nodes
                .iter()
                .filter_map(|node| node.buffer.as_ref())
                .any(|buffer| buffer.lines[0].text() == "1")
        );
        schedule.complete(DrawOutcome::Presented, now + RETRY_DELAY);
        assert_eq!(schedule.control_flow(), ControlFlow::Wait);
    }

    #[test]
    fn consecutive_failures_each_wait_before_retrying() {
        let mut schedule = RedrawSchedule::default();
        let start = Instant::now();
        for attempt in 0..5 {
            let now = start + RETRY_DELAY * attempt;
            schedule.complete(DrawOutcome::Retry, now);
            assert!(!schedule.ready(now));
            assert!(!schedule.take_retry(now + RETRY_DELAY / 2));
            assert_eq!(
                schedule.control_flow(),
                ControlFlow::WaitUntil(now + RETRY_DELAY)
            );
            assert!(schedule.take_retry(now + RETRY_DELAY));
            assert!(!schedule.take_retry(now + RETRY_DELAY));
        }
    }

    #[test]
    fn presentation_clears_retry_and_stops_repeated_redraws() {
        let mut schedule = RedrawSchedule::default();
        let now = Instant::now();
        schedule.complete(DrawOutcome::Retry, now);
        schedule.complete(DrawOutcome::Presented, now + RETRY_DELAY);
        for elapsed in [RETRY_DELAY, RETRY_DELAY * 2, Duration::from_secs(10)] {
            assert!(!schedule.take_retry(now + elapsed));
            assert_eq!(schedule.control_flow(), ControlFlow::Wait);
        }
    }
}
