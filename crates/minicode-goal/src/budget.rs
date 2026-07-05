use crate::types::GoalConfig;
use std::time::Instant;

pub struct BudgetManager {
    pub config: GoalConfig,
    pub tokens_used: usize,
    pub iterations: usize,
    pub consecutive_failures: usize,
    start_time: Instant,
}

impl Clone for BudgetManager {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            tokens_used: self.tokens_used,
            iterations: self.iterations,
            consecutive_failures: self.consecutive_failures,
            start_time: Instant::now(),
        }
    }
}

impl std::fmt::Debug for BudgetManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BudgetManager")
            .field("tokens_used", &self.tokens_used)
            .field("iterations", &self.iterations)
            .field("consecutive_failures", &self.consecutive_failures)
            .field("elapsed_secs", &self.elapsed_secs())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BudgetExceeded {
    MaxIterations,
    MaxDuration,
    MaxConsecutiveFailures,
    MaxTokens,
}

impl std::fmt::Display for BudgetExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BudgetExceeded::MaxIterations => write!(f, "达到最大迭代轮次"),
            BudgetExceeded::MaxDuration => write!(f, "达到最大运行时长"),
            BudgetExceeded::MaxConsecutiveFailures => write!(f, "连续失败次数过多"),
            BudgetExceeded::MaxTokens => write!(f, "达到Token预算上限"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BudgetWarning {
    NearIterationLimit,
    NearDurationLimit,
    NearTokenLimit,
}

impl BudgetManager {
    pub fn new(config: GoalConfig) -> Self {
        Self {
            config,
            tokens_used: 0,
            iterations: 0,
            consecutive_failures: 0,
            start_time: Instant::now(),
        }
    }

    pub fn tick(&mut self) {
        self.iterations += 1;
    }

    pub fn add_tokens(&mut self, tokens: usize) {
        self.tokens_used += tokens;
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
    }

    pub fn reset_failures(&mut self) {
        self.consecutive_failures = 0;
    }

    pub fn check_exceeded(&self) -> Option<BudgetExceeded> {
        if self.iterations >= self.config.max_iterations {
            return Some(BudgetExceeded::MaxIterations);
        }
        if self.start_time.elapsed().as_secs() >= self.config.max_duration_secs {
            return Some(BudgetExceeded::MaxDuration);
        }
        if self.consecutive_failures >= self.config.max_consecutive_failures {
            return Some(BudgetExceeded::MaxConsecutiveFailures);
        }
        if let Some(max_tok) = self.config.max_budget_tokens {
            if self.tokens_used >= max_tok {
                return Some(BudgetExceeded::MaxTokens);
            }
        }
        None
    }

    pub fn check_warning(&self) -> Option<BudgetWarning> {
        let iter_ratio = self.iterations as f64 / self.config.max_iterations as f64;
        if iter_ratio > 0.8 {
            return Some(BudgetWarning::NearIterationLimit);
        }
        let dur_ratio =
            self.start_time.elapsed().as_secs() as f64 / self.config.max_duration_secs as f64;
        if dur_ratio > 0.8 {
            return Some(BudgetWarning::NearDurationLimit);
        }
        if let Some(max_tok) = self.config.max_budget_tokens {
            let tok_ratio = self.tokens_used as f64 / max_tok as f64;
            if tok_ratio > 0.8 {
                return Some(BudgetWarning::NearTokenLimit);
            }
        }
        None
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    pub fn elapsed_display(&self) -> String {
        let secs = self.elapsed_secs();
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        format!("{}h{:02}m{:02}s", h, m, s)
    }

    pub fn progress_ratio(&self) -> f64 {
        let iter_ratio = self.iterations as f64 / self.config.max_iterations as f64;
        let dur_ratio =
            self.start_time.elapsed().as_secs() as f64 / self.config.max_duration_secs as f64;
        iter_ratio.max(dur_ratio)
    }
}
