use crate::logger::{LogState, Move};
use std::{borrow::Cow, collections::VecDeque, io::Result, path::PathBuf};
use tokio::{fs, io::AsyncWriteExt};

pub struct History {
    pub path: PathBuf,
    pub values: VecDeque<Cow<'static, str>>,
    pub pos: usize,
    pub max: usize,
}
impl History {
    pub async fn new(path: PathBuf, max: usize) -> Self {
        let file_path = path.join("history.txt");
        let values = if let Ok(true) = fs::try_exists(&file_path).await {
            fs::read_to_string(file_path).await.map_or_else(
                |err| {
                    log::warn!("Failed to load history: {err}");
                    VecDeque::new()
                },
                |history| {
                    history
                        .split('\n')
                        .map(|str| Cow::Owned(str.to_string()))
                        .rev()
                        .collect()
                },
            )
        } else {
            VecDeque::new()
        };
        History {
            path,
            values,
            pos: 0,
            max,
        }
    }
}
impl History {
    pub fn push(&mut self, out: String) {
        if !self.values.is_empty() && self.values[0] == out {
            return;
        }
        self.values.push_front(Cow::Owned(out));
        if self.values.len() >= self.max {
            self.values.drain(self.max..self.values.len());
        }
    }
    pub fn update(state: &mut LogState, dir: Move) -> Result<()> {
        if state.history.values.is_empty() {
            return Ok(());
        }
        let len = state.history.values.len();
        match dir {
            Move::Up => state.history.pos = (state.history.pos + 1) % (len + 1),
            Move::Down if state.history.pos != 0 => state.history.pos -= 1,
            _ => (),
        }
        if state.history.pos == 0 {
            state.reset()?;
            return Ok(());
        }
        let text = state.history.values[state.history.pos - 1].clone();
        state.out.text = text.to_string();
        let length = text.chars().count();
        state.completion.update(&mut state.out, length);
        state.rewrite_input(length, length)?;
        Ok(())
    }
    pub async fn save(&self) -> Result<()> {
        fs::create_dir_all(&self.path).await?;
        let path = self.path.join("history.txt");
        if let Ok(true) = fs::try_exists(&path).await {
            fs::remove_file(&path).await?;
        }
        let mut file = fs::File::create_new(&path).await?;
        for line in self.values.iter().rev() {
            file.write_all(format!("{line}\n").as_bytes()).await?;
        }
        file.set_len(file.metadata().await?.len().saturating_sub(1))
            .await
    }
}
