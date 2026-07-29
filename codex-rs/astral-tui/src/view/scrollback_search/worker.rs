use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::thread::JoinHandle;

use super::IndexedEntry;
use super::ScrollbackMatch;
use super::compile_query;

enum SearchMessage {
    Update {
        corpus: Option<Arc<[IndexedEntry]>>,
        query: String,
        request_generation: u64,
    },
    Stop,
}

#[derive(Debug)]
pub(super) struct SearchResult {
    pub(super) matches: Arc<[ScrollbackMatch]>,
    pub(super) query: String,
    pub(super) request_generation: u64,
}

#[derive(Debug)]
pub(super) struct SearchDaemon {
    requests: mpsc::Sender<SearchMessage>,
    results: mpsc::Receiver<SearchResult>,
    _handle: JoinHandle<()>,
}

impl SearchDaemon {
    pub(super) fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut corpus: Arc<[IndexedEntry]> = Arc::from([]);
            while let Ok(first) = request_rx.recv() {
                let Some(update) = drain_latest(first, &request_rx) else {
                    break;
                };
                if let Some(next_corpus) = update.corpus {
                    corpus = next_corpus;
                }
                let matches = compile_query(&update.query)
                    .ok()
                    .flatten()
                    .map(|matcher| scan_matches(&corpus, &matcher))
                    .unwrap_or_default()
                    .into();
                if result_tx
                    .send(SearchResult {
                        matches,
                        query: update.query,
                        request_generation: update.request_generation,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            requests: request_tx,
            results: result_rx,
            _handle: handle,
        }
    }

    pub(super) fn update(
        &self,
        corpus: Option<Arc<[IndexedEntry]>>,
        query: String,
        request_generation: u64,
    ) -> bool {
        self.requests
            .send(SearchMessage::Update {
                corpus,
                query,
                request_generation,
            })
            .is_ok()
    }

    pub(super) fn latest_result(&self) -> Result<Option<SearchResult>, ()> {
        let mut latest = None;
        loop {
            match self.results.try_recv() {
                Ok(result) => latest = Some(result),
                Err(mpsc::TryRecvError::Empty) => return Ok(latest),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return latest.map(Some).ok_or(());
                }
            }
        }
    }
}

impl Drop for SearchDaemon {
    fn drop(&mut self) {
        let _ = self.requests.send(SearchMessage::Stop);
    }
}

struct PendingUpdate {
    corpus: Option<Arc<[IndexedEntry]>>,
    query: String,
    request_generation: u64,
}

fn drain_latest(
    first: SearchMessage,
    requests: &mpsc::Receiver<SearchMessage>,
) -> Option<PendingUpdate> {
    let mut update = match first {
        SearchMessage::Update {
            corpus,
            query,
            request_generation,
        } => PendingUpdate {
            corpus,
            query,
            request_generation,
        },
        SearchMessage::Stop => return None,
    };
    loop {
        match requests.try_recv() {
            Ok(SearchMessage::Update {
                corpus,
                query,
                request_generation,
            }) => {
                if corpus.is_some() {
                    update.corpus = corpus;
                }
                update.query = query;
                update.request_generation = request_generation;
            }
            Ok(SearchMessage::Stop) => return None,
            Err(mpsc::TryRecvError::Empty) => return Some(update),
            Err(mpsc::TryRecvError::Disconnected) => return None,
        }
    }
}

fn scan_matches(entries: &[IndexedEntry], matcher: &regex::Regex) -> Vec<ScrollbackMatch> {
    let mut matches = Vec::new();
    for entry in entries {
        let mut line_in_entry = 0;
        let mut counted_to = 0;
        for matched in matcher.find_iter(&entry.text) {
            if matched.start() == matched.end() {
                continue;
            }
            line_in_entry += entry.text[counted_to..matched.start()]
                .matches('\n')
                .count();
            counted_to = matched.start();
            matches.push(ScrollbackMatch {
                entry_id: entry.id.clone(),
                line_in_entry,
            });
        }
    }
    matches
}
