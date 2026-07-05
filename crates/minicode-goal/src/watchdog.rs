#[derive(Debug, Clone)]
pub struct Watchdog {
    pub similarity_threshold: f64,
    pub max_no_progress_iterations: usize,
    pub max_repeated_errors: usize,
    iterations_without_completion: usize,
    recent_error_patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StallReason {
    RepetitiveActions,
    NoProgress,
    RepeatedErrors,
}

impl std::fmt::Display for StallReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StallReason::RepetitiveActions => write!(f, "检测到重复动作"),
            StallReason::NoProgress => write!(f, "多轮无进展"),
            StallReason::RepeatedErrors => write!(f, "重复错误"),
        }
    }
}

impl Default for Watchdog {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.65,
            max_no_progress_iterations: 10,
            max_repeated_errors: 3,
            iterations_without_completion: 0,
            recent_error_patterns: Vec::new(),
        }
    }
}

impl Watchdog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_completion(&mut self) {
        self.iterations_without_completion = 0;
        self.recent_error_patterns.clear();
    }

    pub fn record_iteration(&mut self) {
        self.iterations_without_completion += 1;
    }

    pub fn record_error(&mut self, error_type: &str) {
        let pattern = normalize_error(error_type);
        self.recent_error_patterns.push(pattern);
        if self.recent_error_patterns.len() > self.max_repeated_errors + 2 {
            self.recent_error_patterns.remove(0);
        }
    }

    pub fn detect_stall(&self, recent_actions: &[String]) -> Option<StallReason> {
        if recent_actions.len() >= 5 {
            let window = &recent_actions[recent_actions.len() - 5..];
            if self.are_actions_similar(window) {
                return Some(StallReason::RepetitiveActions);
            }
        }

        if self.iterations_without_completion >= self.max_no_progress_iterations {
            return Some(StallReason::NoProgress);
        }

        if self.repeated_error_count() >= self.max_repeated_errors {
            return Some(StallReason::RepeatedErrors);
        }

        None
    }

    fn repeated_error_count(&self) -> usize {
        if self.recent_error_patterns.len() < self.max_repeated_errors {
            return 0;
        }
        let recent = &self.recent_error_patterns
            [self.recent_error_patterns.len() - self.max_repeated_errors..];
        if recent.len() < self.max_repeated_errors {
            return 0;
        }
        let first = &recent[0];
        if recent.iter().all(|e| e == first) {
            self.max_repeated_errors
        } else {
            0
        }
    }

    fn are_actions_similar(&self, actions: &[String]) -> bool {
        let mut all_words: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut common_words: Option<std::collections::HashSet<String>> = None;

        for action in actions {
            let words: std::collections::HashSet<String> = action
                .split_whitespace()
                .map(|w| w.to_lowercase())
                .filter(|w| w.len() > 2)
                .collect();

            all_words.extend(words.iter().cloned());

            common_words = Some(match common_words {
                None => words,
                Some(cw) => cw.intersection(&words).cloned().collect(),
            });
        }

        let common = common_words.unwrap_or_default();
        if all_words.is_empty() {
            return false;
        }
        let similarity = common.len() as f64 / all_words.len() as f64;
        similarity > self.similarity_threshold
    }
}

fn normalize_error(err: &str) -> String {
    err.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect::<String>()
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join(" ")
}
