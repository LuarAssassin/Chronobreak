use crate::{
    render::{DrawOutcome, Renderer, Result},
    ui::{Element, KeyInput, Scene, WheelInput},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize, PhysicalPosition},
    event::{ElementState, Ime, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{CursorIcon, Window, WindowId},
};

const RETRY_DELAY: Duration = Duration::from_millis(100);

fn logical_position(position: PhysicalPosition<f64>, scale: f64) -> (f32, f32) {
    ((position.x / scale) as f32, (position.y / scale) as f32)
}

#[derive(Debug)]
enum UserEvent {
    Messages,
    Accessibility(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}

#[derive(Default)]
struct RedrawSchedule {
    retry_at: Option<Instant>,
    requested: bool,
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

    fn request(&mut self, needed: bool, now: Instant) -> bool {
        if !needed || self.requested || !self.ready(now) {
            return false;
        }
        self.requested = true;
        true
    }

    fn request_scene(&mut self, needed: bool, drawable: bool, now: Instant) -> bool {
        if !drawable {
            *self = Self::default();
            return false;
        }
        self.request(needed, now)
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

    fn with_blink(&self, blink: Option<Instant>) -> ControlFlow {
        match (self.retry_at, blink) {
            (Some(retry), Some(blink)) => ControlFlow::WaitUntil(retry.min(blink)),
            (None, Some(blink)) => ControlFlow::WaitUntil(blink),
            _ => self.control_flow(),
        }
    }
}

pub struct Application {
    adapter: Option<accesskit_winit::Adapter>,
    bridge: crate::accessibility::Bridge,
    accessibility_version: Option<(u64, f64)>,
    semantics: crate::Semantics,
    proxy: Option<winit::event_loop::EventLoopProxy<UserEvent>>,
    renderer: Option<Renderer>,
    window: Option<Arc<Window>>,
    scene: Scene,
    title: String,
    cursor: Option<PhysicalPosition<f64>>,
    occluded: bool,
    error: Option<Box<dyn std::error::Error>>,
    trace: bool,
    redraw_schedule: RedrawSchedule,
    messages_ready: bool,
    ime_disabling: bool,
    ime_geometry: Option<(crate::ui::Bounds, PhysicalPosition<i32>, f64)>,
}

impl Application {
    pub fn new(root: Element) -> Self {
        let semantics = crate::Semantics::default();
        let mut scene = Scene::new(root);
        scene.set_semantics(semantics.clone());
        Self {
            adapter: None,
            bridge: crate::accessibility::Bridge::default(),
            accessibility_version: None,
            semantics,
            proxy: None,
            renderer: None,
            window: None,
            scene,
            title: "Chronobreak".into(),
            cursor: None,
            occluded: false,
            error: None,
            trace: std::env::var_os("CHRONOBREAK_TRACE").is_some(),
            redraw_schedule: RedrawSchedule::default(),
            messages_ready: false,
            ime_disabling: false,
            ime_geometry: None,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// 显式启用统一语义主题；默认保留既有应用的 M4A 配色。
    pub fn theme(mut self, theme: &crate::ThemeController) -> Self {
        self.scene.set_theme(theme.clone());
        self
    }

    /// 安装真正由提交生产、由事件循环消费动作的 UI 线程边界，非 OS 适配器。
    pub fn semantics(mut self, semantics: &crate::Semantics) -> Self {
        self.semantics = semantics.clone();
        self.scene.set_semantics(semantics.clone());
        self
    }

    pub fn run(mut self) -> Result<()> {
        let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
        let proxy = event_loop.create_proxy();
        self.proxy = Some(proxy.clone());
        self.scene.wake.install(Arc::new(move || {
            proxy.send_event(UserEvent::Messages).is_ok()
        }));
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
        let changed = self
            .scene
            .flush(size.width as f32 / scale, size.height as f32 / scale);
        let outcome = self
            .renderer
            .as_mut()
            .unwrap()
            .draw(self.scene.frame(), scale)?;
        self.redraw_schedule.complete(outcome, Instant::now());
        if outcome == DrawOutcome::Presented {
            self.scene.presented();
        }
        if self.trace && outcome == DrawOutcome::Presented {
            let lines: usize = self
                .scene
                .nodes
                .iter()
                .filter_map(|node| node.buffer.as_ref())
                .map(|buffer| buffer.layout_runs().count())
                .sum();
            eprintln!(
                "frame physical={}x{} scale={scale} changed={changed} text_lines={lines} version={} layouts={} formats={}",
                size.width,
                size.height,
                self.scene.geometry_version,
                self.scene.layouts,
                self.scene.formats,
            );
        }
        Ok(())
    }

    fn sync_ime(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let target = self.scene.ime_target();
        if self.scene.ime_owner.is_some() && self.scene.ime_owner != target {
            self.scene.cancel_ime();
            self.scene.ime_owner = None;
            self.scene.ime_ready = false;
            self.ime_disabling = true;
            self.ime_geometry = None;
            window.set_ime_allowed(false);
            wake_ime_requests(window, &self.title);
        } else if !self.ime_disabling && self.scene.ime_owner.is_none() && target.is_some() {
            self.scene.ime_owner = target;
            self.scene.ime_ready = false;
            window.set_ime_allowed(true);
            wake_ime_requests(window, &self.title);
        }
        if self.scene.ime_ready
            && self.scene.ime_owner == self.scene.ime_target()
            && let Some(area) = self.scene.ime_cursor_area()
        {
            let x11 = window.window_handle().is_ok_and(|handle| {
                matches!(
                    handle.as_raw(),
                    RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)
                )
            });
            let origin = window.outer_position().unwrap_or_default();
            let geometry = (area, origin, window.scale_factor());
            if self.ime_geometry == Some(geometry) {
                return;
            }
            if x11
                && self
                    .ime_geometry
                    .is_some_and(|(_, previous, _)| previous != origin)
            {
                window.set_ime_cursor_area(
                    candidate_position(area, false),
                    LogicalSize::new(area.width as f64, area.height as f64),
                );
            }
            let position = candidate_position(area, x11);
            window.set_ime_cursor_area(
                position,
                LogicalSize::new(area.width as f64, area.height as f64),
            );
            self.ime_geometry = Some(geometry);
            wake_ime_requests(window, &self.title);
            if self.trace {
                eprintln!("ime_area logical={area:?} scale={}", window.scale_factor());
            }
        }
    }
}

fn candidate_position(area: crate::ui::Bounds, x11: bool) -> LogicalPosition<f64> {
    LogicalPosition::new(
        area.x as f64,
        (area.y + if x11 { area.height } else { 0.0 }) as f64,
    )
}

fn wake_ime_requests(window: &Window, title: &str) {
    if window.window_handle().is_ok_and(|handle| {
        matches!(
            handle.as_raw(),
            RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)
        )
    }) {
        window.set_title(title);
    }
    window.request_redraw();
}

impl ApplicationHandler<UserEvent> for Application {
    fn user_event(&mut self, _: &ActiveEventLoop, event: UserEvent) {
        if let UserEvent::Accessibility(event) = event {
            if self
                .window
                .as_ref()
                .is_none_or(|window| window.id() != event.window_id)
            {
                return;
            }
            if matches!(
                event.window_event,
                accesskit_winit::WindowEvent::InitialTreeRequested
                    | accesskit_winit::WindowEvent::AccessibilityDeactivated
            ) {
                self.accessibility_version = None;
            }
            if let accesskit_winit::WindowEvent::ActionRequested(request) = event.window_event
                && let Some((id, action)) = self.bridge.request(request)
            {
                self.semantics.perform(id, action);
            }
        }
        self.messages_ready = true;
        if self.trace {
            eprintln!("user_event messages_ready");
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.adapter = None;
        self.scene.close_messages();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let result = (|| -> Result<_> {
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_visible(false)
                        .with_title(&self.title)
                        .with_inner_size(LogicalSize::new(760.0, 520.0)),
                )?,
            );
            let renderer = pollster::block_on(Renderer::new(window.clone()))?;
            Ok((window, renderer))
        })();
        match result {
            Ok((window, renderer)) => {
                self.adapter = Some(accesskit_winit::Adapter::with_event_loop_proxy(
                    event_loop,
                    &window,
                    self.proxy.as_ref().unwrap().clone(),
                ));
                window.set_visible(true);
                if self.redraw_schedule.request(true, Instant::now()) {
                    window.request_redraw();
                }
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
        if let Some(window) = &self.window {
            window.set_ime_allowed(false);
        }
        self.scene.cancel_ime();
        self.scene.ime_owner = None;
        self.scene.ime_ready = false;
        self.ime_disabling = false;
        self.ime_geometry = None;
        self.adapter = None;
        self.bridge = crate::accessibility::Bridge::default();
        self.accessibility_version = None;
        self.renderer = None;
        self.window = None;
        self.cursor = None;
        self.scene.focus_lost();
        self.redraw_schedule = RedrawSchedule::default();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if std::mem::take(&mut self.messages_ready) {
            let count = self.scene.drain_messages();
            if self.trace {
                eprintln!("messages dispatched={count}");
            }
        }
        if let Some(window) = &self.window {
            let size = window.inner_size();
            let scale = window.scale_factor() as f32;
            self.scene
                .flush(size.width as f32 / scale, size.height as f32 / scale);
            self.scene.update_blink(
                Instant::now(),
                !self.occluded && size.width > 0 && size.height > 0,
            );
            window.set_cursor(if self.scene.pointer_cursor() {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });
            if let Some(renderer) = &mut self.renderer {
                renderer.collect_unmounted(&self.scene.text_ids());
            }
            if self.redraw_schedule.request_scene(
                self.scene.needs_present(),
                !self.occluded && size.width > 0 && size.height > 0,
                Instant::now(),
            ) {
                window.request_redraw();
            }
        }
        if let Some(window) = &self.window
            && self.redraw_schedule.take_retry(Instant::now())
            && self.redraw_schedule.request(true, Instant::now())
        {
            window.request_redraw();
        }
        if let (Some(adapter), Some(window)) = (&mut self.adapter, &self.window) {
            let version = (self.semantics.version(), window.scale_factor());
            if self.accessibility_version != Some(version) {
                adapter.update_if_active(|| {
                    self.accessibility_version = Some(version);
                    self.bridge
                        .update(&self.semantics.snapshot(), &self.title, version.1)
                });
            }
        }
        self.sync_ime();
        let control_flow = self.redraw_schedule.with_blink(self.scene.blink_deadline());
        event_loop.set_control_flow(control_flow);
        if self.trace {
            eprintln!("wait {control_flow:?}");
        }
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
        if let Some(adapter) = &mut self.adapter {
            adapter.process_event(window, &event);
        }
        if self.trace
            && matches!(
                event,
                WindowEvent::CursorMoved { .. }
                    | WindowEvent::CursorLeft { .. }
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::Focused(_)
                    | WindowEvent::KeyboardInput { .. }
                    | WindowEvent::ModifiersChanged(_)
                    | WindowEvent::Occluded(_)
                    | WindowEvent::Resized(_)
                    | WindowEvent::Moved(_)
                    | WindowEvent::ScaleFactorChanged { .. }
                    | WindowEvent::Ime(_)
            )
        {
            eprintln!("input {event:?}");
        }
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        if matches!(event, WindowEvent::ScaleFactorChanged { .. }) {
            self.scene.pointer_position(
                self.cursor
                    .map(|position| logical_position(position, window.scale_factor())),
            );
        }
        self.scene
            .flush(size.width as f32 / scale, size.height as f32 / scale);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.scene.cancel_pointer();
                self.renderer
                    .as_mut()
                    .unwrap()
                    .resize(size.width, size.height);
                let scale = window.scale_factor() as f32;
                self.scene
                    .flush(size.width as f32 / scale, size.height as f32 / scale);
                if self.redraw_schedule.request(true, Instant::now()) {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.inner_size();
                self.scene.cancel_pointer();
                self.renderer
                    .as_mut()
                    .unwrap()
                    .resize(size.width, size.height);
                let scale = window.scale_factor() as f32;
                self.scene
                    .flush(size.width as f32 / scale, size.height as f32 / scale);
                if self.redraw_schedule.request(true, Instant::now()) {
                    window.request_redraw();
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded && self.redraw_schedule.request(true, Instant::now()) {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some(position);
                let (x, y) = logical_position(position, window.scale_factor());
                self.scene.pointer_move(x, y);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.scene.cursor_left();
            }
            WindowEvent::Focused(false) => {
                self.cursor = None;
                self.scene.focus_lost();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.scene.modifiers(modifiers.state()),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                self.scene
                    .keyboard(KeyInput::from_winit(event, is_synthetic));
            }
            WindowEvent::Ime(event) => {
                let disabled = event == Ime::Disabled;
                if disabled {
                    self.ime_disabling = false;
                }
                self.scene.ime(event);
                if disabled {
                    self.scene.ime_owner = None;
                    self.ime_geometry = None;
                }
            }
            WindowEvent::Touch(winit::event::Touch {
                phase: winit::event::TouchPhase::Cancelled,
                ..
            }) => {
                self.scene.cancel_pointer();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let scale = window.scale_factor();
                let size = window.inner_size();
                self.scene.flush(
                    (size.width as f64 / scale) as f32,
                    (size.height as f64 / scale) as f32,
                );
                let position = self
                    .cursor
                    .map(|position| logical_position(position, scale));
                self.scene
                    .mouse_input(state == ElementState::Pressed, button, position);
                self.renderer
                    .as_mut()
                    .unwrap()
                    .collect_unmounted(&self.scene.text_ids());
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                self.scene
                    .wheel(WheelInput::from_winit(delta, phase, window.scale_factor()));
            }
            WindowEvent::RedrawRequested => {
                self.redraw_schedule.requested = false;
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

    #[test]
    fn candidate_coordinates_are_window_local_logical_and_x11_uses_caret_bottom() {
        let area = crate::ui::Bounds {
            x: 52.0,
            y: 83.0,
            width: 1.5,
            height: 27.0,
        };
        assert_eq!(
            candidate_position(area, false),
            LogicalPosition::new(52.0, 83.0)
        );
        assert_eq!(
            candidate_position(area, true),
            LogicalPosition::new(52.0, 110.0)
        );
        for scale in [1.0, 1.5] {
            let physical = candidate_position(area, true).to_physical::<i32>(scale);
            assert_eq!(
                physical,
                PhysicalPosition::new(
                    (52.0 * scale).round() as i32,
                    (110.0 * scale).round() as i32
                )
            );
        }
    }

    #[test]
    fn blink_and_surface_retry_choose_earliest_deadline_without_idle_polling() {
        let now = Instant::now();
        let mut schedule = RedrawSchedule::default();
        assert_eq!(schedule.with_blink(None), ControlFlow::Wait);
        assert_eq!(schedule.with_blink(Some(now)), ControlFlow::WaitUntil(now));
        schedule.complete(DrawOutcome::Retry, now);
        assert_eq!(
            schedule.with_blink(Some(now + Duration::from_secs(1))),
            ControlFlow::WaitUntil(now + RETRY_DELAY)
        );
        assert_eq!(
            schedule.with_blink(Some(now + Duration::from_millis(1))),
            ControlFlow::WaitUntil(now + Duration::from_millis(1))
        );
        schedule.complete(DrawOutcome::Presented, now + RETRY_DELAY);
        assert_eq!(schedule.with_blink(None), ControlFlow::Wait);
    }
    use crate::signal::Signal;

    #[test]
    fn backend_pointer_normalization_retains_negative_coordinates_and_dpi() {
        assert_eq!(
            logical_position(PhysicalPosition::new(300.0, 150.0), 1.5),
            (200.0, 100.0)
        );
        assert_eq!(
            logical_position(PhysicalPosition::new(-3.0, -6.0), 1.5),
            (-2.0, -4.0)
        );
    }

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

    #[test]
    fn redraw_intent_coalesces_and_retry_presents_latest_commit() {
        let value = Signal::new(0);
        let mut scene = Scene::new(Element::bound_text(&value, |value| value.to_string()));
        let mut schedule = RedrawSchedule::default();
        let now = Instant::now();
        scene.flush(400.0, 600.0);
        assert!(schedule.request(scene.needs_present(), now));
        assert!(!schedule.request(true, now));
        schedule.requested = false;
        schedule.complete(DrawOutcome::Retry, now);
        value.update(|value| *value = 2);
        scene.flush(400.0, 600.0);
        assert!(!schedule.request(scene.needs_present(), now));
        assert!(schedule.take_retry(now + RETRY_DELAY));
        assert!(schedule.request(scene.needs_present(), now + RETRY_DELAY));
        assert_eq!(scene.geometry_version, scene.commit_version);
        assert_eq!(
            scene
                .nodes
                .iter()
                .next()
                .unwrap()
                .buffer
                .as_ref()
                .unwrap()
                .lines[0]
                .text(),
            "2"
        );
        schedule.requested = false;
        schedule.complete(DrawOutcome::Presented, now + RETRY_DELAY);
        scene.presented();
        assert!(!schedule.request(scene.needs_present(), now + RETRY_DELAY));
        assert_eq!(schedule.control_flow(), ControlFlow::Wait);
    }

    #[test]
    fn zero_size_or_occlusion_pauses_retry_without_losing_scene_intent() {
        let mut scene = Scene::new(Element::text("pending"));
        scene.flush(400.0, 600.0);
        let now = Instant::now();
        let mut schedule = RedrawSchedule::default();
        schedule.complete(DrawOutcome::Retry, now);
        for _ in 0..5 {
            assert!(!schedule.request_scene(scene.needs_present(), false, now));
            assert!(!schedule.take_retry(now + RETRY_DELAY));
            assert_eq!(schedule.control_flow(), ControlFlow::Wait);
            assert!(scene.needs_present());
        }
        assert!(schedule.request_scene(scene.needs_present(), true, now + RETRY_DELAY));
        assert!(!schedule.request_scene(scene.needs_present(), true, now + RETRY_DELAY));
        scene.presented();
        schedule.requested = false;
        schedule.complete(DrawOutcome::Presented, now + RETRY_DELAY);
        assert!(!schedule.request_scene(scene.needs_present(), true, now + RETRY_DELAY));
    }
}
