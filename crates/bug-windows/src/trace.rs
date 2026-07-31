//! Stable, line-oriented frame tracing used by Wine and Windows VM checks.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use bug_runtime::contract::{BodyState, Decision, MotionFeedback};

pub const TRACE_HEADER: &str = concat!(
    "frame\tinstance\tstate\tbody_x\tbody_y\theading\tspeed\t",
    "target_x\ttarget_y\tdisplacement_x\tdisplacement_y\t",
    "blocked_time\tedge_dwell_time\trecovery_x\trecovery_y\t",
    "recovery_clearance\trng_draws\tconsume_bait\tquarantined\n"
);

#[derive(Clone, Copy, Debug)]
pub struct TraceRow<'decision> {
    pub frame_index: u64,
    pub instance_id: u64,
    pub decision: &'decision Decision,
    pub body: BodyState,
    pub feedback: MotionFeedback,
    pub rng_draws: u64,
    pub quarantined: bool,
}

pub struct TraceWriter {
    writer: BufWriter<File>,
}

impl TraceWriter {
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(TRACE_HEADER.as_bytes())?;
        Ok(Self { writer })
    }

    pub fn write_row(&mut self, row: TraceRow<'_>) -> io::Result<()> {
        self.writer.write_all(format_trace_row(row).as_bytes())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

/// Formats one row without locale-sensitive APIs.  Rust's finite-float
/// display is shortest-roundtrip, which keeps the file compact while
/// preserving every f32 value needed by replay comparisons.
#[must_use]
pub fn format_trace_row(row: TraceRow<'_>) -> String {
    let decision = row.decision;
    let body = row.body;
    let feedback = row.feedback;
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        row.frame_index,
        row.instance_id,
        sanitized_field(&decision.state),
        body.position.x,
        body.position.y,
        body.heading,
        body.speed,
        decision.target.x,
        decision.target.y,
        feedback.actual_displacement.x,
        feedback.actual_displacement.y,
        feedback.blocked_time,
        feedback.edge_dwell_time,
        feedback.recovery_direction.x,
        feedback.recovery_direction.y,
        feedback.recovery_clearance,
        row.rng_draws,
        u8::from(decision.consume_bait),
        u8::from(row.quarantined),
    )
}

#[must_use]
fn sanitized_field(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' | '\0' => '\u{fffd}',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bug_runtime::math::Vec2;

    #[test]
    fn trace_row_has_one_record_and_sanitizes_script_text() {
        let decision = Decision {
            state: "seek\tfood\n".to_owned(),
            target: Vec2::new(10.5, -3.25),
            ..Decision::default()
        };
        let row = format_trace_row(TraceRow {
            frame_index: 4,
            instance_id: 2,
            decision: &decision,
            body: BodyState {
                position: Vec2::new(1.0, 2.0),
                heading: 0.25,
                speed: 88.0,
                length: 165.0,
            },
            feedback: MotionFeedback::default(),
            rng_draws: 17,
            quarantined: false,
        });
        assert!(row.starts_with("4\t2\tseek\u{fffd}food\u{fffd}\t1\t2\t0.25\t88\t"));
        assert_eq!(row.matches('\n').count(), 1);
        assert_eq!(row.trim_end().split('\t').count(), 19);
    }
}
