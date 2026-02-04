use crate::logger::{LogState, Move, output::Output};
use std::{borrow::Cow, collections::VecDeque, io::Result};
use tokio::{fs, io::AsyncWriteExt};

pub struct History {
    pub path: &'static str,
    pub values: VecDeque<Cow<'static, str>>,
    pub pos: usize,
}
impl History {
    pub async fn new(path: &'static str) -> Self {
        let file_path = format!("{path}/history.txt");
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
        }
    }
}
impl History {
    pub fn push(&mut self, out: &Output) {
        if !self.values.is_empty() && self.values[0] == out.text {
            return;
        }
        self.values.push_front(Cow::Owned(out.text.clone()));
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
        fs::create_dir_all(self.path).await?;
        let path = format!("{}/history.txt", self.path);
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
