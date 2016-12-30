use super::structs::SearchResult;
use utils::Dimension;
use clap;
use std::str;
use std::sync::{Mutex, Arc};
use std::thread;
use futures_cpupool::CpuPool;
use curl::easy::Easy;
use termion::event::Key;
use termion::raw::IntoRawMode;
use termion::input::TermRead;
use termion::clear;
use termion::cursor;
use tokio_core::reactor::Core;
use futures::{Sink, Stream, Future};
use futures::sync::mpsc;
use tokio_curl::Session;
use std::io::{self, Write};
use std::fmt::Display;

use utils::ok_or_exit;

const INFO_LINE: cursor::Goto = cursor::Goto(1, 2);
const CONTENT_LINE: cursor::Goto = cursor::Goto(1, 3);
const NON_CONTENT_LINES: u16 = 2;

fn dimension() -> Dimension {
    Dimension::default().loose_heigth(NON_CONTENT_LINES)
}

#[derive(Clone, Copy)]
enum Mode {
    Searching
}

use self::Mode::*;

#[derive(Clone)]
struct State  {
    mode: Mode,
    term: String
}

impl Default for State {
    fn default() -> State {
       State {
           mode: Searching,
          term: String::new()
       }
    }
}

pub fn handle_interactive_search(_args: &clap::ArgMatches) {
    let stdin = io::stdin();
    let mut stdout = ok_or_exit(io::stdout().into_raw_mode());
    ok_or_exit(write!(stdout, "{}{}", cursor::Goto(1, 1), clear::All));
    promptf("", &mut stdout);
    usage();

    let (sender, receiver) = mpsc::channel(10);

    let t = thread::spawn(|| {
        let mut reactor = ok_or_exit(Core::new());
        let session = Session::new(reactor.handle());
        let mut current_result = None;

        let operations = receiver.and_then(|state: State| {
            match state.mode {
                Searching => {
                    let mut req = Easy::new();
                    let dim = dimension();
                    ok_or_exit(req.get( true ));
                    let url = format !("https://crates.io/api/v1/crates?page=1&per_page={}&q={}&sort=",
                                       dim.height,
                                       req.url_encode(String::as_bytes( &state.term)));
                    ok_or_exit(req.url( & url));
                    let buf = Arc::new(Mutex::new(Vec::new()));
                    let buf_handle = buf.clone();
                    ok_or_exit(req.write_function( move | data | {
                        buf_handle.lock().unwrap().extend_from_slice(data);
                        Ok(data.len())
                    }));
                    info( & "searching ...");
                    session.perform(req)
                           .map_err( | e| {
                               info( & e);
                               ()
                           })
                           .map( move | _response | {
                               let buf_slice = buf.lock().unwrap();
                               let result = SearchResult::from_data(& buf_slice, dim).map_err( | e| {
                                   write ! (io::stderr(), "{}\n", String::from_utf8_lossy( & buf_slice)).ok();
                                   e
                               });
                               result
                           })
                }}
            })
            .for_each(|search| {
                let search: SearchResult = ok_or_exit(search);
                info(&format!("{} results in total, showing {} max",
                              search.meta.total,
                              search.meta.dimension.as_ref().expect("dimension to be set").height));
                if search.crates.is_empty() {
                    let last = usage();
                    write!(io::stdout(),
                           "{gotolast} - 0 results found",
                           gotolast = cursor::Goto(last as u16, INFO_LINE.1))
                        .ok();
                } else {
                    write!(io::stdout(), "{goto}{}", search, goto = CONTENT_LINE).ok();
                    current_result = Some(search);
                }
                io::stdout().flush().ok();
                Ok(())
            });
        reactor.run(operations).ok();
    });

    let mut ongoing_search = None;
    let mut state = State::default();
    let pool = CpuPool::new(1);

    for k in stdin.keys() {
        match state.mode {
            Searching => {
                match ok_or_exit(k) {
                    Key::Char('\n') => {
                        state.term.clear();
                    }
                    Key::Char(c) => {
                        state.term.push(c);
                    }
                    Key::Backspace => {
                        state.term.pop();
                    }
                    Key::Esc => {
                        break;
                    }
                    key @ _ => {
                        info(&format!("unsupported key sequence: {:?}", key));
                        continue;
                    }
                }
                promptf(&state.term, &mut stdout);
                if state.term.is_empty() {
                    usage();
                    write!(stdout,
                           "{goto}{}",
                           SearchResult::with_dimension(dimension()),
                           goto = CONTENT_LINE)
                        .ok();
                    stdout.flush().ok();
                } else {
                    ongoing_search = Some(pool.spawn(sender.clone().send(state.clone())));
                }
            }
        }
    }
    drop(ongoing_search);
    drop(sender);
    t.join().unwrap();
    reset_terminal();
}

fn reset_terminal() {
    write!(io::stdout(),
           "{}{}{}",
           cursor::Goto(1, 1),
           cursor::Show,
           clear::All)
        .ok();
}

fn usage() -> usize {
    info(&"(<ESC> to quit, <enter> to clear, Ctrl+o to open) Please enter your search term.")
}

fn info(item: &Display) -> usize {
    let buf = format!("{}", item);
    write!(io::stdout(),
           "{hide}{goto}{clear}{}",
           buf,
           hide = cursor::Hide,
           goto = INFO_LINE,
           clear = clear::CurrentLine)
        .ok();
    io::stdout().flush().ok();
    buf.len()
}

fn promptf(term: &str, stdout: &mut io::Stdout) {
    write!(stdout,
           "{show}{goto}{clear}crates.io: {}",
           term,
           show = cursor::Show,
           goto = cursor::Goto(1, 1),
           clear = clear::CurrentLine)
        .ok();
    stdout.flush().ok();

}
