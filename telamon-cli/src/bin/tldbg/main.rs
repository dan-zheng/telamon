use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::{fs, io, thread};

use log::{debug, warn};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use tui::backend::TermionBackend;
use tui::buffer::Buffer;
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, Borders, List, Paragraph, SelectableList, Text, Widget};
use tui::Terminal;

use telamon::codegen;
use telamon::device::{Context, EvalMode, KernelEvaluator};
use telamon::explorer::{
    self,
    choice::{self, ActionEx as Action},
    find_best_ex, Candidate,
};
use telamon::ir::IrDisplay;
use telamon::model::{bound, Bound};
use telamon::search_space::SearchSpace;
use telamon_cli::{Bench, CublasHandle, KernelBundle, KernelParam, Platform, ReplayPath};
use telamon_cuda;
use telamon_kernels::statistics::estimate_mean;
use telamon_kernels::{linalg, Kernel, KernelBuilder};

use crossbeam::channel;
use futures::{executor, Future};
use structopt::StructOpt;

trait Dispatch<'a, E> {
    fn add_listener<F>(&mut self, f: F)
    where
        F: FnMut(&E) -> bool + 'a;

    fn with_listener<F>(mut self, f: F) -> Self
    where
        F: FnMut(&E) -> bool + 'a,
        Self: Sized,
    {
        self.add_listener(f);
        self
    }
}

trait InputHandler<E> {
    type Output;

    fn handle_input(&mut self, event: &E) -> Option<Self::Output>;
}

trait InputHandlerExt<E>: InputHandler<E> + Sized {
    fn or_else<U>(self, other: U) -> OrElse<Self, U>
    where
        U: InputHandler<E, Output = Self::Output>,
    {
        OrElse { a: self, b: other }
    }
}

struct OrElse<A, B> {
    a: A,
    b: B,
}

impl<A, B, E, O> InputHandler<E> for OrElse<A, B>
where
    A: InputHandler<E, Output = O>,
    B: InputHandler<E, Output = O>,
{
    type Output = O;

    fn handle_input(&mut self, event: &E) -> Option<O> {
        let OrElse { a, b } = self;
        a.handle_input(event).or_else(move || b.handle_input(event))
    }
}

impl<E, O, F> InputHandler<E> for F
where
    F: FnMut(&E) -> Option<O>,
{
    type Output = O;

    fn handle_input(&mut self, event: &E) -> Option<O> {
        self(event)
    }
}

impl<T, E> InputHandler<E> for Option<T>
where
    T: InputHandler<E>,
{
    type Output = T::Output;

    fn handle_input(&mut self, event: &E) -> Option<Self::Output> {
        self.as_mut()
            .and_then(move |inner| inner.handle_input(event))
    }
}

struct InputDispatcher<'a, E> {
    handlers: Vec<Box<dyn FnMut(&E) -> bool + 'a>>,
}

impl<'a, E> InputDispatcher<'a, E> {
    pub fn new() -> Self {
        InputDispatcher {
            handlers: Vec::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        InputDispatcher {
            handlers: Vec::with_capacity(capacity),
        }
    }

    pub fn dispatch(&mut self, event: &E) -> bool {
        // Iterate in reverse order to ensure last added handlers have higher priority
        for handler in self.handlers.iter_mut().rev() {
            if handler(event) {
                return true;
            }
        }

        return false;
    }

    pub fn clear(&mut self) {
        self.handlers.clear();
    }
}

impl<'a, E> Dispatch<'a, E> for InputDispatcher<'a, E> {
    fn add_listener<F>(&mut self, f: F)
    where
        F: FnMut(&E) -> bool + 'a,
    {
        self.handlers.push(Box::new(f))
    }
}

struct IgnoreDispatch;

impl<'a, E> Dispatch<'a, E> for IgnoreDispatch {
    fn add_listener<F>(&mut self, _f: F)
    where
        F: FnMut(&E) -> bool + 'a,
    {
    }
}

trait Ignore {
    fn ignore(self);
}

impl<T, E> Ignore for Result<T, E> {
    fn ignore(self) {}
}

struct Node {
    children: Vec<Edge>,
    bound: Bound,
    bound_compute_time: std::time::Duration,
    candidate: SearchSpace,
    evaluations: RwLock<Vec<Option<f64>>>,
    benchmarks: RwLock<Vec<f64>>,
}

impl Node {
    fn new(candidate: SearchSpace, env: &dyn Env) -> Self {
        let children = env
            .list_actions(&candidate)
            .into_iter()
            .map(|action| Edge {
                node: RwLock::new(None),
                action,
            })
            .collect::<Vec<_>>();
        let start = std::time::Instant::now();
        let bound = env.bound(&candidate);
        let duration = start.elapsed();
        Node {
            children,
            bound,
            bound_compute_time: duration,
            candidate,
            evaluations: RwLock::new(Vec::new()),
            benchmarks: RwLock::new(Vec::new()),
        }
    }

    fn is_implementation(&self) -> bool {
        self.children.is_empty()
    }

    fn get_or_compute_edge(&self, index: usize, env: &dyn Env) -> &Edge {
        let edge = &self.children[index];
        edge.get_or_compute_node(&self.candidate, env);
        edge
    }
}

struct Edge {
    node: RwLock<Option<Option<Arc<Node>>>>,
    action: Action,
}

impl Edge {
    fn get_or_compute_node(
        &self,
        parent: &SearchSpace,
        env: &dyn Env,
    ) -> Option<Arc<Node>> {
        if let Some(node_ref) = &*self.node.read().expect("node: poisoned") {
            return node_ref.as_ref().map(Arc::clone);
        }

        let node_ref = &mut *self.node.write().expect("node: poisoned");
        match node_ref {
            Some(node_ref) => node_ref.as_ref().map(Arc::clone),
            None => {
                let start = std::time::Instant::now();
                let node = env.apply_action(parent.clone(), self.action.clone());
                let duration = start.elapsed();
                debug!("propagation took {:?}", duration);

                let node = node.map(|c| Arc::new(Node::new(c, env)));
                *node_ref = Some(node.as_ref().map(Arc::clone));
                node
            }
        }
    }

    fn node(&self) -> Result<Option<Arc<Node>>, ()> {
        match &*self.node.read().expect("node: poisoned") {
            None => Err(()),
            Some(node_ref) => Ok(node_ref.as_ref().map(Arc::clone)),
        }
    }
}

struct Cursor<'a> {
    env: &'a dyn Env,
    path: Vec<(Arc<Node>, usize)>,
    node: Arc<Node>,
}

impl<'a> Cursor<'a> {
    fn new(env: &'a dyn Env, node: Arc<Node>) -> Self {
        Cursor {
            env,
            node,
            path: Vec::new(),
        }
    }

    fn select_action(&mut self, action: &Action) -> Result<(), ()> {
        let index = self
            .node
            .children
            .iter()
            .enumerate()
            .find(|(i, e)| e.action == *action)
            .map(|(i, _)| i)
            .unwrap_or_else(|| {
                panic!(
                    "Unable to find action {}",
                    action.display(self.node.candidate.ir_instance())
                )
            });

        self.select_child(index)
    }

    fn select_child(&mut self, index: usize) -> Result<(), ()> {
        if let Some(child) = self
            .node
            .get_or_compute_edge(index, self.env)
            .node()
            .unwrap()
        {
            self.path
                .push((std::mem::replace(&mut self.node, child), index));

            Ok(())
        } else {
            Err(())
        }
    }

    fn compute_bound(&self, index: usize) {
        self.node.get_or_compute_edge(index, self.env);
    }

    fn undo(&mut self) -> Result<(), ()> {
        if let Some((node, _)) = self.path.pop() {
            self.node = node;
            Ok(())
        } else {
            Err(())
        }
    }

    fn path(&self) -> impl Iterator<Item = &Action> {
        self.path
            .iter()
            .map(|(node, index)| &node.children[*index].action)
    }
}

struct TuiCursor<'a> {
    cursor: Cursor<'a>,
    actions: Vec<(usize, String)>,
    action_pos: usize,
    filter: Option<String>,
}

impl<'a> TuiCursor<'a> {
    fn new(cursor: Cursor<'a>) -> Self {
        TuiCursor {
            actions: Self::get_action_strings(&cursor.node),
            action_pos: 0,
            filter: None,
            cursor,
        }
    }

    fn get_action_strings(node: &Node) -> Vec<(usize, String)> {
        node.children
            .iter()
            .enumerate()
            .map(|(ix, edge)| {
                (
                    ix,
                    format!(
                        "{} {}",
                        edge.node()
                            .map(|node| match node {
                                None => "[INVALID]".to_string(),
                                Some(node) => format!("[{:<7.2e}]", node.bound.value()),
                            })
                            .unwrap_or_else(|_| "[???????]".to_string()),
                        edge.action.display(node.candidate.ir_instance()),
                    ),
                )
            })
            .collect()
    }

    fn down(&mut self) -> Result<(), ()> {
        if self.action_pos < self.filtered_actions().count() {
            self.action_pos += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    fn up(&mut self) -> Result<(), ()> {
        if self.action_pos > 0 {
            self.action_pos -= 1;
            Ok(())
        } else {
            Err(())
        }
    }

    fn update(&mut self) -> Result<(), ()> {
        self.actions = Self::get_action_strings(&self.cursor.node);
        self.action_pos = 0;
        self.filter = None;
        Ok(())
    }

    fn select(&mut self) -> Result<(), ()> {
        self.cursor
            .select_child(self.selected_action().ok_or(())?.0)?;
        self.update()
    }

    fn undo(&mut self) -> Result<(), ()> {
        self.cursor.undo()?;
        self.update()
    }

    fn compute_bound(&mut self) -> Result<(), ()> {
        self.cursor
            .compute_bound(self.selected_action().ok_or(())?.0);
        self.actions = Self::get_action_strings(&self.cursor.node);
        Ok(())
    }

    fn unfilter(&mut self) {
        self.action_pos = 0;
        self.filter = None;
    }

    fn filter(&mut self, filter: String) {
        self.action_pos = 0;
        self.filter = Some(filter);
    }

    fn filtered_actions(&self) -> impl Iterator<Item = &(usize, String)> + '_ {
        self.actions.iter().filter(move |(_, s)| {
            if let Some(filter) = self.filter.as_ref() {
                s.contains(filter)
            } else {
                true
            }
        })
    }

    fn selected_action(&self) -> Option<&(usize, String)> {
        self.filtered_actions().nth(self.action_pos)
    }
}

impl<'a> Widget for TuiCursor<'a> {
    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        let path_strings = self
            .cursor
            .path
            .iter()
            .enumerate()
            .map(|(id, (node, index))| {
                Text::raw(format!(
                    "{:>3}. [{:.2e}ns] {}",
                    id,
                    node.children[*index].node().unwrap().unwrap().bound.value(),
                    node.children[*index]
                        .action
                        .display(&node.candidate.ir_instance()),
                ))
            })
            .collect::<Vec<_>>();

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Min(60)].as_ref())
            .split(area);

        if self.cursor.node.is_implementation() {
            let code = codegen::Function::build(&self.cursor.node.candidate);
            Paragraph::new([Text::raw(format!("{}", code))].iter())
                .wrap(false)
                .draw(chunks[0], buf);
        } else {
            let action_strings =
                self.filtered_actions().map(|(_, s)| s).collect::<Vec<_>>();
            SelectableList::default()
                .block(
                    Block::default()
                        .title("Available actions")
                        .borders(Borders::ALL),
                )
                .items(&action_strings)
                .select(Some(self.action_pos))
                .highlight_style(Style::default().modifier(Modifier::ITALIC))
                .highlight_symbol(">>")
                .draw(chunks[0], buf);
        }

        List::new(path_strings.into_iter())
            .block(Block::default().title("Path").borders(Borders::ALL))
            .draw(chunks[1], buf);
    }
}

struct Explorer<'a> {
    actionline: Option<String>,
    selector: TuiCursor<'a>,
}

impl<'a> Explorer<'a> {
    fn new(selector: TuiCursor<'a>) -> Self {
        Explorer {
            actionline: None,
            selector,
        }
    }
}

impl<'a> Widget for Explorer<'a> {
    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        let modeline = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
        let rest = Rect::new(area.x, area.y, area.width, area.height - 1);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Ratio(2, 3), Constraint::Ratio(1, 3)].as_ref())
            .split(rest);

        self.selector.draw(chunks[0], buf);
        let evaluations = self
            .selector
            .cursor
            .node
            .evaluations
            .read()
            .unwrap()
            .iter()
            .filter_map(|&x| x)
            .collect::<Vec<_>>();
        let estimate = if evaluations.len() < 2 {
            None
        } else {
            Some(estimate_mean(evaluations, 0.95, "ns"))
        };
        let benchmarks = self.selector.cursor.node.benchmarks.read().unwrap().clone();
        Paragraph::new(
            [
                Text::raw(format!(
                    "[computed in {:?}] {}\n",
                    self.selector.cursor.node.bound_compute_time,
                    self.selector.cursor.node.bound,
                )),
                Text::raw(if let Some(estimate) = estimate {
                    format!("estimate: {}\n", estimate)
                } else {
                    "".to_string()
                }),
                Text::raw(if benchmarks.is_empty() {
                    "".to_string()
                } else {
                    format!("benchmark: {}\n", estimate_mean(benchmarks, 0.95, "ns"))
                }),
                Text::raw(format!(
                    "{}\n",
                    self.selector.cursor.node.candidate.ir_instance()
                )),
            ]
            .iter(),
        )
        .wrap(true)
        .draw(chunks[1], buf);

        if let Some(actionline) = &self.actionline {
            Paragraph::new([Text::raw(actionline)].iter())
                .wrap(false)
                .style(Style::default().bg(Color::Gray))
                .draw(modeline, buf);
        }

        /* Alert - WIP
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(50),
                    Constraint::Min(4),
                    Constraint::Percentage(50),
                ]
                .as_ref(),
            )
            .split(area);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(15),
                    Constraint::Min(40),
                    Constraint::Percentage(15),
                ]
                .as_ref(),
            )
            .split(chunks[1]);

        Paragraph::new([Text::raw("Replay saved to /tmp/wut.json")].iter())
            .wrap(true)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .title("File saved")
                    .title_style(Style::default().bg(Color::Gray))
                    .borders(Borders::ALL)
                    .border_style(Style::default().bg(Color::Gray)),
            )
            .style(Style::default().bg(Color::Gray))
            .draw(chunks[1], buf);
            */
    }
}

struct FullEnv<'a, O> {
    context: &'a dyn Context,
    order: O,
}

pub trait Env {
    fn list_actions(&self, candidate: &SearchSpace) -> Vec<Action>;

    fn apply_action(&self, candidate: SearchSpace, action: Action)
        -> Option<SearchSpace>;

    fn bound(&self, candidate: &SearchSpace) -> Bound;
}

impl<'a, O> Env for FullEnv<'a, O>
where
    O: Iterator<Item = &'a explorer::config::ChoiceGroup> + Clone + 'a,
{
    fn list_actions(&self, candidate: &SearchSpace) -> Vec<Action> {
        // We need the map to cast lifetimes
        choice::list(self.order.clone().map(|x| x), candidate)
            .flat_map(|x| x.into_iter())
            .collect()
    }

    fn apply_action(
        &self,
        candidate: SearchSpace,
        action: Action,
    ) -> Option<SearchSpace> {
        action.apply_to(candidate).ok()
    }

    fn bound(&self, candidate: &SearchSpace) -> Bound {
        bound(candidate, self.context)
    }
}

fn keys_stream() -> futures::sync::mpsc::UnboundedReceiver<io::Result<Key>> {
    let (sender, receiver) = futures::sync::mpsc::unbounded();

    thread::Builder::new()
        .name("tldbg - input".to_string())
        .spawn(move || {
            for c in termion::get_tty().unwrap().keys() {
                if let Err(_) = sender.unbounded_send(c) {
                    break;
                }
            }
        })
        .unwrap();

    receiver
}

trait EvaluationFn: Send {
    type Output;

    fn call(
        self: Box<Self>,
        candidate: Candidate,
        kernel: &mut dyn KernelEvaluator,
    ) -> Self::Output;
}

impl<F, T> EvaluationFn for F
where
    F: FnOnce(Candidate, &mut dyn KernelEvaluator) -> T + Send,
{
    type Output = T;

    fn call(
        self: Box<Self>,
        candidate: Candidate,
        kernel: &mut dyn KernelEvaluator,
    ) -> Self::Output {
        (*self)(candidate, kernel)
    }
}

struct Evaluator<'a, T = f64> {
    sender: channel::Sender<(
        Candidate,
        Box<dyn EvaluationFn<Output = T> + 'a>,
        futures::sync::oneshot::Sender<T>,
    )>,
}

impl<'a, T: Send> Evaluator<'a, T> {
    fn scoped<F, R>(context: &'a Context, num_workers: usize, mode: EvalMode, f: F) -> R
    where
        F: FnOnce(&Self) -> R,
    {
        crossbeam::scope(move |s| {
            let (sender, receiver) = channel::unbounded();
            let evaluator = Evaluator { sender };

            s.builder()
                .name("tldbg - evaluator".to_string())
                .spawn(move |_| {
                    context.async_eval(num_workers, mode, &move |async_evaluator| {
                        while let Ok((candidate, evaluation_fn, sender)) = receiver.recv()
                        {
                            async_evaluator.add_kernel(
                                candidate,
                                move |candidate, kevaluator| {
                                    if sender
                                        .send(evaluation_fn.call(candidate, kevaluator))
                                        .is_err()
                                    {
                                        println!("future was dropped.")
                                    }
                                },
                            )
                        }
                    })
                })
                .unwrap();

            f(&evaluator)
        })
        .unwrap()
    }

    fn evaluate<F>(
        &self,
        candidate: Candidate,
        evaluation_fn: F,
    ) -> futures::sync::oneshot::Receiver<T>
    where
        F: FnOnce(Candidate, &mut dyn KernelEvaluator) -> T + Send + 'a,
    {
        let (sender, receiver) = futures::sync::oneshot::channel();
        self.sender
            .send((candidate, Box::new(evaluation_fn), sender))
            .unwrap();
        receiver
    }
}

trait AssertSend: Send {}

impl AssertSend for codegen::Function<'_> {}

/// The Telamon Debugger
#[derive(Debug, StructOpt)]
#[structopt(name = "tldbg")]
struct Opt {
    /// Path to the replay directory.
    ///
    /// The replay directory is used to store replays with 'w', which can later be reloaded with
    /// `--replay`.
    ///
    /// Replays are stored as .json files containing the actions to use, which is also the same
    /// format used by the replay tests.
    #[structopt(parse(from_os_str), long = "replay-dir", alias = "replay_dir")]
    replay_dir: Option<PathBuf>,

    /// Path to a replay file to load.
    ///
    /// Warning: It is up to the user to ensure that the replay file is compatible with the
    /// provided kernel!
    ///
    /// The replay file is a .json file containing a serialized representation of the actions to
    /// apply, as saved by the 'w' command in the debugger or the replay tests.
    #[structopt(parse(from_os_str), long = "replay")]
    replay: Option<ReplayPath>,

    /// Kernel specification to use.
    #[structopt(long = "kernel", default_value = "matmul_256_256_32")]
    kernel: KernelParam,

    #[structopt(long = "platform", default_value = "cuda")]
    platform: Platform,

    #[structopt(long = "order")]
    order: Option<explorer::config::ChoiceOrdering>,
}

impl Opt {
    pub fn save_replay(&self, replay: &[Action]) -> io::Result<Option<PathBuf>> {
        if let Some(path) = &self.replay_dir {
            // Ensure the replay directory exists
            fs::create_dir_all(path)?;

            let names = fs::read_dir(path)?
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect::<Result<HashSet<_>, _>>()?;

            let mut ix = names.len();
            let name = loop {
                let name = std::ffi::OsString::from(format!("replay{}.json", ix));
                if !names.contains(&name) {
                    break name;
                } else {
                    ix += 1;
                }
            };

            let full_path = path.join(&name);
            fs::write(&full_path, serde_json::to_string(replay)?)?;
            Ok(Some(full_path))
        } else {
            warn!("Trying to save replay but no replay directory was defined.");

            Ok(None)
        }
    }
}

fn main() -> io::Result<()> {
    env_logger::init();

    let args = Opt::from_args();

    #[cfg(feature = "cuda")]
    let (cuda_executor, mut cuda_context);

    #[cfg(feature = "x86")]
    let mut x86_context;

    let (
        KernelBundle {
            candidates,
            check_fn,
            reference_fn,
        },
        context,
    ) = match args.platform {
        Platform::X86 => {
            #[cfg(feature = "x86")]
            {
                x86_context = telamon_x86::Context::default();
                let reference = telamon_cli::X86Reference::default();

                let (bundle, context) =
                    args.kernel.to_bundle(&mut x86_context, reference);
                (bundle, context as &dyn Context)
            }

            #[cfg(not(feature = "x86"))]
            {
                panic!("not supported")
            }
        }
        Platform::Cuda => {
            #[cfg(feature = "cuda")]
            {
                cuda_executor = telamon_cuda::Executor::init();
                cuda_context = telamon_cuda::Context::new(&cuda_executor);
                let reference = CublasHandle::new();

                let (bundle, context) =
                    args.kernel.to_bundle(&mut cuda_context, reference);
                (bundle, context as &dyn Context)
            }

            #[cfg(not(feature = "cuda"))]
            {
                panic!("not supported")
            }
        }
    };

    let default_order = explorer::config::ChoiceOrdering::default();
    let order: &explorer::config::ChoiceOrdering =
        args.order.as_ref().unwrap_or(&default_order);
    let env = FullEnv {
        context,
        order: order.into_iter(),
    };

    let stabilizer = &context.stabilizer();
    let mut config = explorer::Config::default();
    match &mut config.algorithm {
        explorer::SearchAlgorithm::Mcts(bconfig) => {
            if let Some(order) = &args.order {
                bconfig.choice_ordering = order.clone();
            }
        }
        _ => unreachable!(),
    }
    config.output_dir = "/tmp".to_string();
    config.max_evaluations = Some(10);

    let check_fn = &check_fn;
    Evaluator::<Result<f64, String>>::scoped(
        context,
        1,
        EvalMode::FindBest,
        |evaluator| {
            let candidate = candidates[0].space.clone();
            let children = env
                .list_actions(&candidate)
                .into_iter()
                .map(|action| Edge {
                    node: RwLock::new(None),
                    action,
                })
                .collect::<Vec<_>>();
            let start = std::time::Instant::now();
            let bound = env.bound(&candidate);
            let duration = start.elapsed();
            let root = Arc::new(Node {
                children,
                bound,
                bound_compute_time: duration,
                candidate: candidate,
                evaluations: RwLock::new(Vec::new()),
                benchmarks: RwLock::new(Vec::new()),
            });

            let stdout = io::stdout().into_raw_mode()?;
            let stdin = io::stdin();
            let backend = TermionBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;

            terminal.hide_cursor()?;
            terminal.clear()?;

            let mut widget = Explorer::new(TuiCursor::new(Cursor::new(&env, root)));

            if let Some(replay) = &args.replay {
                for action in &replay.load()? {
                    widget.selector.cursor.select_action(action).unwrap();
                }
            }

            terminal.draw(|mut f| {
                let size = f.size();
                widget.render(&mut f, size);
            })?;

            let mut command = String::new();
            for c in stdin.keys() {
                widget.actionline = None;
                {
                    let mut dispatcher = InputDispatcher::new();

                    if command.is_empty() {
                        dispatcher.add_listener(|key| {
                            let mut handled = true;
                            match key {
                                Key::Char('\n') => {
                                    widget.selector.select().ignore();
                                }
                                Key::Up | Key::Char('k') => widget.selector.up().ignore(),
                                Key::Down | Key::Char('j') => {
                                    widget.selector.down().ignore()
                                }
                                Key::Char('/') => command.push('/'),
                                Key::Char('u') => widget.selector.undo().ignore(),
                                Key::Char('b') => {
                                    let node = &widget.selector.cursor.node;
                                    if node.is_implementation() {
                                        let code =
                                            codegen::Function::build(&node.candidate);
                                        let runtimes = context.benchmark(&code, 40);
                                        node.benchmarks.write().unwrap().extend(runtimes);
                                    } else {
                                        widget.selector.compute_bound().ignore()
                                    }
                                }
                                Key::Char('w') => {
                                    if args.replay_dir.is_some() {
                                        let actions: Vec<_> = widget
                                            .selector
                                            .cursor
                                            .path()
                                            .cloned()
                                            .collect();
                                        let path =
                                            args.save_replay(&actions).unwrap().unwrap();
                                        widget.actionline = Some(format!(
                                            "Replay saved to `{}`",
                                            path.display()
                                        ));
                                    } else {
                                        widget.actionline = Some(
                                            "No replay directory available.".to_string(),
                                        );
                                    }
                                }
                                Key::Char('s') => {
                                    let candidates = vec![Candidate::new(
                                        widget.selector.cursor.node.candidate.clone(),
                                        widget.selector.cursor.node.bound.clone(),
                                    )];

                                    if let Some(best) = find_best_ex(
                                        &config,
                                        context,
                                        candidates,
                                        Some(&check_fn),
                                    ) {
                                        for action in best.actions.into_iter() {
                                            eprintln!(
                                                "Trying {}",
                                                action.display(
                                                    widget
                                                        .selector
                                                        .cursor
                                                        .node
                                                        .candidate
                                                        .ir_instance()
                                                )
                                            );

                                            widget
                                                .selector
                                                .cursor
                                                .select_action(&action)
                                                .unwrap();
                                        }
                                    }
                                }
                                Key::Char('r') => {
                                    let ref_runtime = Bench::default()
                                        .warmup(4)
                                        .runs(40)
                                        .benchmark_fn(&reference_fn);
                                    widget.actionline = Some(format!(
                                        "reference runtime: {}",
                                        estimate_mean(ref_runtime, 0.95, "ns")
                                    ));
                                }
                                Key::Char('c') => {
                                    let node = &widget.selector.cursor.node;
                                    if !node.is_implementation() {
                                        return false;
                                    }

                                    let code = codegen::Function::build(&node.candidate);
                                    context.device().print(
                                        &code,
                                        &mut std::fs::File::create(Path::new(
                                            "/tmp/code.c",
                                        ))
                                        .unwrap(),
                                    );

                                    let candidate = Candidate::new(
                                        node.candidate.clone(),
                                        node.bound.clone(),
                                    );

                                    let node = Arc::clone(node);
                                    executor::spawn(
                                        evaluator
                                            .evaluate(
                                                candidate,
                                                move |candidate, keval| {
                                                    let runtime = stabilizer
                                                        .wrap(keval)
                                                        .evaluate()
                                                        .unwrap();
                                                    check_fn(&candidate, context)?;
                                                    Ok(runtime)
                                                },
                                            )
                                            .map(|eval| {
                                                node.evaluations
                                                    .write()
                                                    .unwrap()
                                                    .push(eval.ok());
                                            }),
                                    )
                                    .wait_future()
                                    .unwrap()
                                }
                                Key::Ctrl('l') => {
                                    // Yes, we need both of those because `clear` doesn't do what you think
                                    // it does (it does not update termion's buffers), and `draw` does not
                                    // clear the screen but only the portions which were changed (which
                                    // includes manually erasing characters in this case -- crazy, I know).
                                    terminal.draw(|_| {}).unwrap();
                                    terminal.clear().unwrap();
                                }
                                Key::Esc => widget.selector.unfilter(),
                                /*
                                Key::Ctrl('r') => widget.selector.redo().ignore(),
                                */
                                _ => handled = false,
                            }
                            handled
                        });
                    } else {
                        dispatcher.add_listener(|key| {
                            let mut handled = true;
                            match key {
                                Key::Up => widget.selector.up().ignore(),
                                Key::Down => widget.selector.down().ignore(),
                                Key::Char('\n') => command = "".to_string(),
                                Key::Char(c) => {
                                    command.push(*c);
                                    widget.selector.filter(command[1..].to_string());
                                }
                                _ => handled = false,
                            }
                            handled
                        })
                    };

                    let event = c?;

                    if !dispatcher.dispatch(&event) {
                        match &event {
                            Key::Char('q') => break,
                            _ => (),
                        }
                    }
                }

                terminal.draw(|mut f| {
                    let size = f.size();
                    widget.render(&mut f, size);
                })?;
            }

            terminal.clear()?;
            std::mem::drop(terminal);

            //println!("Selected: {:?}", selected.map(|s| s.bound.value()));
            Ok(())
        },
    )
}
