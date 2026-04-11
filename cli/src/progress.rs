// Copyright 2022-2026 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{Ordering, AtomicI64};
use std::time::Duration;
use std::time::SystemTime;

use crossterm::terminal::Clear;
use crossterm::terminal::ClearType;
use jj_lib::repo_path::RepoPath;

use crate::text_util;
use crate::ui::OutputGuard;
use crate::ui::ProgressOutput;
use crate::ui::Ui;

pub const UPDATE_HZ: u32 = 30;
pub const INITIAL_DELAY: Duration = Duration::from_millis(250);

struct Inner {
    guard: Option<OutputGuard>,
    output: ProgressOutput<io::Stderr>,
}

fn time_to_ms(time: SystemTime) -> i64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

pub struct ProgressWriter<'a> {
    prefix: &'a str,
    next_display_time: AtomicI64,
    inner: Mutex<Inner>,
}

// Future work: Make that the progress prints the current element we are
// currently working on, either:
// - upon change of the element we are working on and if the next display time
//   is passed (current behavior)
// - upon reaching the next display time for an element that would have not been
//   displayed yet. This
// would assure two things:
// - The first message will end-up being displayed if it takes more than the
//   initial delay to process the associated element. Assuring jj never goes
//   silent for more than the specified initial delay.
// - For the other elements, what we print is more factual regarding what we are
//   doing. Without printing too much.
//
// Note that the first message printing by itself, would not be suitable for the
// commit signing progress as, on some configuration the user is prompted to
// enter its password to unlock the key used to sign, then the message would be
// conflicting with that message from another process.

impl<'a> ProgressWriter<'a> {
    pub fn new(ui: &Ui, prefix: &'a str) -> Option<Self> {
        let output = ui.progress_output()?;

        // Don't clutter the output during fast operations.
        let next_display_time = AtomicI64::new(time_to_ms(SystemTime::now() + INITIAL_DELAY));
        Some(Self {
            prefix,
            next_display_time,
            inner: Mutex::new(Inner {
                guard: None,
                output,
            }),
        })
    }

    pub fn display2(&self, text: &str) -> io::Result<()> {
        let now = SystemTime::now();
        if time_to_ms(now) < self.next_display_time.load(Ordering::Relaxed) {
            return Ok(());
        }

        // or don't use try here...
        let Ok(mut inner) = self.inner.try_lock() else {
            return Ok(());
        };
        
        self.next_display_time.store(time_to_ms(now + Duration::from_secs(1) / UPDATE_HZ), Ordering::Relaxed);

        if inner.guard.is_none() {
            inner.guard = Some(
                inner.output
                    .output_guard(format!("\r{}", Clear(ClearType::CurrentLine))),
            );
        }

        let line_width = inner.output.term_width().map(usize::from).unwrap_or(80);
        let max_path_width = self.prefix.len() + 1; // Take into account the empty space added after the prefix.
        let (display_text, _) =
            text_util::elide_start(text, "...", line_width.saturating_sub(max_path_width));

        write!(
            inner.output,
            "\r{}{} {display_text}",
            Clear(ClearType::CurrentLine),
            self.prefix
        )?;
        inner.output.flush()
    }

    pub fn display(&mut self, text: &str) -> io::Result<()> {
        let now = SystemTime::now();
        if time_to_ms(now) < self.next_display_time.load(Ordering::Relaxed) {
            return Ok(());
        }

        let inner = self.inner.get_mut().expect("poisoned");
        
        *self.next_display_time .get_mut() = time_to_ms(now + Duration::from_secs(1) / UPDATE_HZ);

        if inner.guard.is_none() {
            inner.guard = Some(
                inner.output
                    .output_guard(format!("\r{}", Clear(ClearType::CurrentLine))),
            );
        }

        let line_width = inner.output.term_width().map(usize::from).unwrap_or(80);
        let max_path_width = self.prefix.len() + 1; // Take into account the empty space added after the prefix.
        let (display_text, _) =
            text_util::elide_start(text, "...", line_width.saturating_sub(max_path_width));

        write!(
            inner.output,
            "\r{}{} {display_text}",
            Clear(ClearType::CurrentLine),
            self.prefix
        )?;
        inner.output.flush()
    }
}

pub fn snapshot_progress(ui: &Ui) -> Option<impl Fn(&RepoPath) + use<>> {
    let writer = ProgressWriter::new(ui, "Snapshotting")?;

    Some(move |path: &RepoPath| {
        writer
            .display2(path.to_fs_path_unchecked(Path::new("")).to_str().unwrap())
            .ok();
    })
}
