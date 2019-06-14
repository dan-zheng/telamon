use std::cmp::Ord;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::{fs, io, thread};

use rayon::prelude::*;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use tui::backend::TermionBackend;
use tui::buffer::Buffer;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Modifier, Style};
use tui::widgets::{Block, Borders, List, Paragraph, SelectableList, Text, Widget};
use tui::Terminal;

use log::warn;
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
use telamon_cuda;
use telamon_kernels::{linalg, Kernel, KernelBuilder};

use crossbeam::channel;
use futures::{executor, Future};
use quicli::prelude::{CliResult, Verbosity};
use structopt::StructOpt;

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
                let node = env
                    .apply_action(parent.clone(), self.action.clone())
                    .map(|c| Arc::new(Node::new(c, env)));
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
        let mut edges = node.children.iter().enumerate().collect::<Vec<_>>();
        edges.sort_unstable_by(|(_, lhs), (_, rhs)| lhs.action.cmp(&rhs.action));

        edges
            .into_iter()
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
    selector: TuiCursor<'a>,
}

impl<'a> Explorer<'a> {
    fn new(selector: TuiCursor<'a>) -> Self {
        Explorer { selector }
    }
}

impl<'a> Widget for Explorer<'a> {
    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Min(40)].as_ref())
            .split(area);

        self.selector.draw(chunks[0], buf);
        Paragraph::new(
            [
                Text::raw(format!(
                    "{} (computed in {:?})\n",
                    self.selector.cursor.node.bound,
                    self.selector.cursor.node.bound_compute_time
                )),
                Text::raw(format!(
                    "{:?}",
                    self.selector
                        .cursor
                        .node
                        .evaluations
                        .read()
                        .unwrap()
                        .iter()
                        .map(|e| if let Some(e) = e {
                            e.to_string()
                        } else {
                            "ERR".to_string()
                        })
                        .collect::<Vec<_>>()
                )),
                Text::raw(format!(
                    "{}\n",
                    self.selector.cursor.node.candidate.ir_instance()
                )),
            ]
            .iter(),
        )
        .wrap(true)
        .draw(chunks[1], buf);
    }
}

struct FullEnv<'a> {
    context: &'a dyn Context,
}

pub trait Env {
    fn list_actions(&self, candidate: &SearchSpace) -> Vec<Action>;

    fn apply_action(&self, candidate: SearchSpace, action: Action)
        -> Option<SearchSpace>;

    fn bound(&self, candidate: &SearchSpace) -> Bound;
}

impl<'a> Env for FullEnv<'a> {
    fn list_actions(&self, candidate: &SearchSpace) -> Vec<Action> {
        choice::default_list(candidate)
            .flat_map(|x| x.into_iter())
            .collect()
    }

    fn apply_action(
        &self,
        mut candidate: SearchSpace,
        action: Action,
    ) -> Option<SearchSpace> {
        if let Ok(()) = match action {
            Action::Action(action) => candidate.apply_decisions(vec![action]),
            Action::LowerLayout {
                mem,
                st_dims,
                ld_dims,
            } => candidate.lower_layout(mem, &st_dims, &ld_dims),
        } {
            Some(candidate)
        } else {
            None
        }
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

/// The Telamon Debugger
#[derive(Debug, StructOpt)]
#[structopt(name = "tldbg")]
struct Opt {
    /// Path to the replay directory.
    ///
    /// The replay directory is used to store replays with 'w', which can later be reloaded with
    /// 'r' (not yet implemented).
    ///
    /// Replays are stored as .json files containing the actions to use, which is also the same
    /// format used by the replay tests.
    #[structopt(
        parse(from_os_str),
        long = "replay-dir",
        raw(aliases = r#"&["replay_dir"]"#)
    )]
    replay_dir: Option<PathBuf>,

    // From quicli
    #[structopt(flatten)]
    verbosity: Verbosity,
}

impl Opt {
    pub fn save_replay(&self, replay: &[Action]) -> io::Result<()> {
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

            fs::write(path.join(&name), serde_json::to_string(replay)?)
        } else {
            warn!("Trying to save replay but no replay directory was defined.");

            Ok(())
        }
    }
}

fn main() -> CliResult {
    let args = Opt::from_args();
    args.verbosity.setup_env_logger("telamon")?;

    let executor = telamon_cuda::Executor::init();
    let mut context = telamon_cuda::Context::new(&executor);
    let params = linalg::FusedMMP::new(256, 256, 256);

    let (signature, kernel, context) = KernelBuilder::default()
        .build::<linalg::FusedMM<f32>, telamon_cuda::Context>(
            params.clone(),
            &mut context,
        );
    let signature = Arc::new(signature);
    let expected = kernel.get_expected_output(context);
    let env = FullEnv { context };

    let stabilizer = &context.stabilizer();
    let mut config = explorer::Config::default();
    config.output_dir = "/tmp".to_string();
    config.max_evaluations = Some(10);

    let checker =
        |_: &Candidate, context: &dyn Context| kernel.check_result(&expected, context);

    Evaluator::<Result<f64, String>>::scoped(
        context,
        1,
        EvalMode::FindBest,
        |evaluator| {
            let candidate = kernel
                .build_body(signature, context)
                .swap_remove(0)
                .space
                .prioritized();
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
            });

            let stdout = io::stdout().into_raw_mode()?;
            let stdin = io::stdin();
            let backend = TermionBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;

            terminal.hide_cursor()?;
            terminal.clear()?;

            let mut widget = Explorer::new(TuiCursor::new(Cursor::new(&env, root)));

            terminal.draw(|mut f| {
                let size = f.size();
                widget.render(&mut f, size);
            })?;

            let mut command = String::new();
            for c in stdin.keys() {
                if command.is_empty() {
                    match c? {
                        Key::Char('q') => break,
                        Key::Char('\n') => {
                            widget.selector.select().ignore();
                        }
                        Key::Up | Key::Char('k') => widget.selector.up().ignore(),
                        Key::Down | Key::Char('j') => widget.selector.down().ignore(),
                        Key::Char('/') => command.push('/'),
                        Key::Char('u') => widget.selector.undo().ignore(),
                        Key::Char('b') => widget.selector.compute_bound().ignore(),
                        Key::Char('w') => {
                            let actions: Vec<_> =
                                widget.selector.cursor.path().cloned().collect();
                            args.save_replay(&actions).unwrap();
                        }
                        Key::Char('s') => {
                            let candidates = vec![Candidate::new(
                                widget.selector.cursor.node.candidate.clone(),
                                widget.selector.cursor.node.bound.clone(),
                            )];

                            if let Some(best) =
                                find_best_ex(&config, context, candidates, Some(&checker))
                            {
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

                                    let action_index = widget
                                        .selector
                                        .cursor
                                        .node
                                        .children
                                        .iter()
                                        .enumerate()
                                        .find(|(i, e)| e.action == *action)
                                        .map(|(i, _)| i)
                                        .unwrap_or_else(|| {
                                            panic!(
                                                "Unable to find action {}",
                                                action.display(
                                                    widget
                                                        .selector
                                                        .cursor
                                                        .node
                                                        .candidate
                                                        .ir_instance()
                                                )
                                            );
                                        });
                                    widget
                                        .selector
                                        .cursor
                                        .select_child(action_index)
                                        .unwrap();
                                }
                            }
                        }
                        Key::Char('c') => {
                            let node = &widget.selector.cursor.node;
                            if !node.is_implementation() {
                                continue;
                            }

                            let candidate = Candidate::new(
                                node.candidate.clone(),
                                node.bound.clone(),
                            );

                            let node = Arc::clone(node);
                            executor::spawn(
                                evaluator
                                    .evaluate(candidate, move |candidate, keval| {
                                        let runtime =
                                            stabilizer.wrap(keval).evaluate().unwrap();
                                        checker(&candidate, context)?;
                                        Ok(runtime)
                                    })
                                    .map(|eval| {
                                        node.evaluations.write().unwrap().push(eval.ok());
                                    }),
                            )
                            .wait_future()
                            .unwrap()
                        }
                        Key::Ctrl('l') => (),
                        Key::Esc => widget.selector.unfilter(),
                        /*
                        Key::Ctrl('r') => widget.selector.redo().ignore(),
                        */
                        _ => (),
                    }
                } else {
                    match c? {
                        Key::Up => widget.selector.up().ignore(),
                        Key::Down => widget.selector.down().ignore(),
                        Key::Char('\n') => command = "".to_string(),
                        Key::Char(c) => {
                            command.push(c);
                            widget.selector.filter(command[1..].to_string());
                        }
                        _ => (),
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
