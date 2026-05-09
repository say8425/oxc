use std::{
    borrow::Cow,
    sync::Mutex,
    time::{Duration, Instant},
};

use rustc_hash::FxHashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleTimingSource {
    Native,
    TypeAware,
    JsPlugin,
}

impl RuleTimingSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::TypeAware => "type-aware",
            Self::JsPlugin => "js-plugin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RuleTimingKey {
    source: RuleTimingSource,
    plugin_name: Cow<'static, str>,
    rule_name: Cow<'static, str>,
}

impl RuleTimingKey {
    fn native(plugin_name: &'static str, rule_name: &'static str) -> Self {
        Self {
            source: RuleTimingSource::Native,
            plugin_name: Cow::Borrowed(plugin_name),
            rule_name: Cow::Borrowed(rule_name),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RuleTimingStat {
    pub duration: Duration,
    pub calls: Option<u64>,
}

impl RuleTimingStat {
    fn add(&mut self, other: Self) {
        let is_empty = self.duration.is_zero() && self.calls.is_none();
        self.duration += other.duration;
        self.calls = if is_empty {
            other.calls
        } else {
            match (self.calls, other.calls) {
                (Some(left), Some(right)) => Some(left + right),
                _ => None,
            }
        };
    }

    #[inline]
    pub(crate) fn time<F>(&mut self, f: F)
    where
        F: FnOnce(),
    {
        let start = Instant::now();
        f();
        self.add(Self { duration: start.elapsed(), calls: Some(1) });
    }
}

#[derive(Debug, Clone)]
pub struct RuleTimingRecord {
    pub source: RuleTimingSource,
    pub plugin_name: String,
    pub rule_name: String,
    pub duration: Duration,
    pub calls: Option<u64>,
}

#[expect(
    clippy::redundant_pub_crate,
    reason = "recorder is shared with generated dispatch through the crate root"
)]
#[derive(Debug, Default)]
pub(crate) struct RuleTimingRecorder {
    timings: FxHashMap<RuleTimingKey, RuleTimingStat>,
}

impl RuleTimingRecorder {
    #[inline]
    pub(crate) fn record_native(
        &mut self,
        plugin_name: &'static str,
        rule_name: &'static str,
        stat: RuleTimingStat,
    ) {
        self.timings.entry(RuleTimingKey::native(plugin_name, rule_name)).or_default().add(stat);
    }

    fn into_timings(self) -> FxHashMap<RuleTimingKey, RuleTimingStat> {
        self.timings
    }
}

#[derive(Debug, Default)]
pub struct RuleTimingStore {
    timings: Mutex<FxHashMap<RuleTimingKey, RuleTimingStat>>,
}

impl RuleTimingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn merge(&self, recorder: RuleTimingRecorder) {
        let local_timings = recorder.into_timings();
        if local_timings.is_empty() {
            return;
        }

        let mut timings = self.timings.lock().expect("rule timing store mutex poisoned");
        for (key, stat) in local_timings {
            timings.entry(key).or_default().add(stat);
        }
    }

    /// Collects all rule timings sorted by descending duration.
    ///
    /// # Panics
    /// Panics if the rule timing mutex is poisoned.
    pub fn collect(&self) -> Vec<RuleTimingRecord> {
        let timings = self.timings.lock().expect("rule timing store mutex poisoned");
        let mut records = timings
            .iter()
            .map(|(key, stat)| RuleTimingRecord {
                source: key.source,
                plugin_name: key.plugin_name.to_string(),
                rule_name: key.rule_name.to_string(),
                duration: stat.duration,
                calls: stat.calls,
            })
            .collect::<Vec<_>>();

        records.sort_unstable_by(|left, right| {
            right
                .duration
                .cmp(&left.duration)
                .then_with(|| left.source.as_str().cmp(right.source.as_str()))
                .then_with(|| left.plugin_name.cmp(&right.plugin_name))
                .then_with(|| left.rule_name.cmp(&right.rule_name))
        });
        records
    }
}
