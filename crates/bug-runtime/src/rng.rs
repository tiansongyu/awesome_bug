//! Stable, tagged pseudo-random streams used by Lua controllers.
//!
//! This module deliberately does not use `rand`.  The engine, seed derivation,
//! and integer-to-float mapping are part of the runtime's replay contract.

use std::error::Error;
use std::fmt;

const MT_STATE_WORDS: usize = 624;
const MT_PERIOD_OFFSET: usize = 397;
const MT_MATRIX_A: u32 = 0x9908_b0df;
const MT_UPPER_MASK: u32 = 0x8000_0000;
const MT_LOWER_MASK: u32 = 0x7fff_ffff;
const SPLITMIX_GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;
const UNIT_F32_SCALE: f32 = 1.0 / 16_777_216.0;
const MAX_TAG_BYTES: usize = 256;

/// The project-owned MT19937 implementation.
///
/// Its output matches the original 32-bit MT19937 reference implementation
/// initialized with `init_genrand(seed)`.
#[derive(Clone, Debug)]
pub struct Mt19937 {
    state: [u32; MT_STATE_WORDS],
    index: usize,
}

impl Mt19937 {
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn new(seed: u32) -> Self {
        let mut state = [0_u32; MT_STATE_WORDS];
        state[0] = seed;
        for index in 1..MT_STATE_WORDS {
            let previous = state[index - 1];
            state[index] = 1_812_433_253_u32
                .wrapping_mul(previous ^ (previous >> 30))
                .wrapping_add(index as u32);
        }
        Self {
            state,
            index: MT_STATE_WORDS,
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.index >= MT_STATE_WORDS {
            self.twist();
        }

        let mut value = self.state[self.index];
        self.index += 1;
        value ^= value >> 11;
        value ^= (value << 7) & 0x9d2c_5680;
        value ^= (value << 15) & 0xefc6_0000;
        value ^= value >> 18;
        value
    }

    fn twist(&mut self) {
        for index in 0..MT_STATE_WORDS {
            let joined = (self.state[index] & MT_UPPER_MASK)
                | (self.state[(index + 1) % MT_STATE_WORDS] & MT_LOWER_MASK);
            let mut twisted = joined >> 1;
            if joined & 1 != 0 {
                twisted ^= MT_MATRIX_A;
            }
            self.state[index] = self.state[(index + MT_PERIOD_OFFSET) % MT_STATE_WORDS] ^ twisted;
        }
        self.index = 0;
    }
}

/// SplitMix64 used solely to derive independent, deterministic stream seeds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(SPLITMIX_GAMMA);
        mix_splitmix64(self.state)
    }
}

const fn mix_splitmix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Derives the MT19937 seed for a numbered stream.
///
/// Stream zero is reserved for spawning.  Controller instance `n` uses stream
/// `n + 1`.  Folding both halves prevents the low half of SplitMix64 from
/// becoming an accidental second, undocumented seed contract.
#[must_use]
pub fn derive_seed(master_seed: u64, stream: u64) -> u32 {
    let input = master_seed.wrapping_add(SPLITMIX_GAMMA.wrapping_mul(stream.wrapping_add(1)));
    let mixed = mix_splitmix64(input);
    (mixed as u32) ^ (mixed >> 32) as u32
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamSeeds {
    pub spawn: u32,
    pub instances: Vec<u32>,
}

#[must_use]
pub fn derive_stream_seeds(master_seed: u64, instance_count: usize) -> StreamSeeds {
    StreamSeeds {
        spawn: derive_seed(master_seed, 0),
        instances: (0..instance_count)
            .map(|index| {
                let stream = u64::try_from(index).unwrap_or(u64::MAX);
                derive_seed(master_seed, stream.wrapping_add(1))
            })
            .collect(),
    }
}

/// One replayable draw.  Float values are stored as bits so signed zero and
/// every other IEEE-754 detail are checked exactly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RandomSample {
    pub tag: String,
    pub low_bits: u32,
    pub high_bits: u32,
    pub value_bits: u32,
}

impl RandomSample {
    #[must_use]
    pub fn new(tag: impl Into<String>, low: f32, high: f32, value: f32) -> Self {
        Self {
            tag: tag.into(),
            low_bits: low.to_bits(),
            high_bits: high.to_bits(),
            value_bits: value.to_bits(),
        }
    }

    #[must_use]
    pub const fn low(&self) -> f32 {
        f32::from_bits(self.low_bits)
    }

    #[must_use]
    pub const fn high(&self) -> f32 {
        f32::from_bits(self.high_bits)
    }

    #[must_use]
    pub const fn value(&self) -> f32 {
        f32::from_bits(self.value_bits)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RandomError {
    InvalidTagLength {
        bytes: usize,
    },
    InvalidRange {
        low_bits: u32,
        high_bits: u32,
    },
    TapeEnded {
        draw: u64,
    },
    TapeMismatch {
        draw: u64,
        expected_tag: String,
        requested_tag: String,
        expected_low_bits: u32,
        requested_low_bits: u32,
        expected_high_bits: u32,
        requested_high_bits: u32,
    },
    InvalidTapeValue {
        draw: u64,
        value_bits: u32,
    },
    TapeRemaining {
        samples: usize,
    },
}

impl fmt::Display for RandomError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTagLength { bytes } => {
                write!(
                    formatter,
                    "RNG tag must contain 1..={MAX_TAG_BYTES} bytes, got {bytes}"
                )
            }
            Self::InvalidRange {
                low_bits,
                high_bits,
            } => write!(
                formatter,
                "RNG range must be finite, ordered f32 values (low=0x{low_bits:08x}, high=0x{high_bits:08x})"
            ),
            Self::TapeEnded { draw } => {
                write!(formatter, "RNG tape ended at draw {draw}")
            }
            Self::TapeMismatch {
                draw,
                expected_tag,
                requested_tag,
                expected_low_bits,
                requested_low_bits,
                expected_high_bits,
                requested_high_bits,
            } => write!(
                formatter,
                "RNG tape mismatch at draw {draw}: expected {expected_tag:?} \
                 [0x{expected_low_bits:08x}, 0x{expected_high_bits:08x}], requested \
                 {requested_tag:?} [0x{requested_low_bits:08x}, 0x{requested_high_bits:08x}]"
            ),
            Self::InvalidTapeValue { draw, value_bits } => write!(
                formatter,
                "RNG tape contains an invalid value at draw {draw}: 0x{value_bits:08x}"
            ),
            Self::TapeRemaining { samples } => {
                write!(formatter, "RNG tape has {samples} unconsumed sample(s)")
            }
        }
    }
}

impl Error for RandomError {}

#[derive(Clone, Debug)]
enum RandomMode {
    Generate {
        engine: Box<Mt19937>,
        record: bool,
        tape: Vec<RandomSample>,
    },
    Replay {
        tape: Vec<RandomSample>,
        index: usize,
    },
}

/// A random stream whose semantic call order is verified by string tags.
#[derive(Clone, Debug)]
pub struct TaggedRng {
    mode: RandomMode,
    draw_count: u64,
}

impl TaggedRng {
    #[must_use]
    pub fn generate(seed: u32) -> Self {
        Self {
            mode: RandomMode::Generate {
                engine: Box::new(Mt19937::new(seed)),
                record: false,
                tape: Vec::new(),
            },
            draw_count: 0,
        }
    }

    #[must_use]
    pub fn recording(seed: u32) -> Self {
        Self {
            mode: RandomMode::Generate {
                engine: Box::new(Mt19937::new(seed)),
                record: true,
                tape: Vec::new(),
            },
            draw_count: 0,
        }
    }

    #[must_use]
    pub fn replay(tape: Vec<RandomSample>) -> Self {
        Self {
            mode: RandomMode::Replay { tape, index: 0 },
            draw_count: 0,
        }
    }

    /// Draws an f32 using the fixed mapping
    /// `(word >> 8) * 2^-24`, followed by separately rounded f32 multiply and
    /// add operations: `low + (high - low) * unit`.
    pub fn draw(&mut self, tag: &str, low: f32, high: f32) -> Result<f32, RandomError> {
        validate_request(tag, low, high)?;

        let value = match &mut self.mode {
            RandomMode::Generate {
                engine,
                record,
                tape,
            } => {
                let value = map_word_to_range(engine.next_u32(), low, high);
                if *record {
                    tape.push(RandomSample::new(tag, low, high, value));
                }
                value
            }
            RandomMode::Replay { tape, index } => {
                let Some(sample) = tape.get(*index) else {
                    return Err(RandomError::TapeEnded {
                        draw: self.draw_count,
                    });
                };
                if sample.tag != tag
                    || sample.low_bits != low.to_bits()
                    || sample.high_bits != high.to_bits()
                {
                    return Err(RandomError::TapeMismatch {
                        draw: self.draw_count,
                        expected_tag: sample.tag.clone(),
                        requested_tag: tag.to_owned(),
                        expected_low_bits: sample.low_bits,
                        requested_low_bits: low.to_bits(),
                        expected_high_bits: sample.high_bits,
                        requested_high_bits: high.to_bits(),
                    });
                }
                let value = sample.value();
                if !value.is_finite() || value < low || value > high {
                    return Err(RandomError::InvalidTapeValue {
                        draw: self.draw_count,
                        value_bits: sample.value_bits,
                    });
                }
                *index += 1;
                value
            }
        };

        self.draw_count += 1;
        Ok(value)
    }

    #[must_use]
    pub const fn draw_count(&self) -> u64 {
        self.draw_count
    }

    #[must_use]
    pub fn tape(&self) -> &[RandomSample] {
        match &self.mode {
            RandomMode::Generate { tape, .. } | RandomMode::Replay { tape, .. } => tape,
        }
    }

    #[must_use]
    pub fn replay_complete(&self) -> bool {
        match &self.mode {
            RandomMode::Replay { tape, index } => *index == tape.len(),
            RandomMode::Generate { .. } => true,
        }
    }

    pub fn require_replay_complete(&self) -> Result<(), RandomError> {
        match &self.mode {
            RandomMode::Replay { tape, index } if *index != tape.len() => {
                Err(RandomError::TapeRemaining {
                    samples: tape.len() - index,
                })
            }
            _ => Ok(()),
        }
    }
}

fn validate_request(tag: &str, low: f32, high: f32) -> Result<(), RandomError> {
    if tag.is_empty() || tag.len() > MAX_TAG_BYTES {
        return Err(RandomError::InvalidTagLength { bytes: tag.len() });
    }
    if !low.is_finite() || !high.is_finite() || low > high || !(high - low).is_finite() {
        return Err(RandomError::InvalidRange {
            low_bits: low.to_bits(),
            high_bits: high.to_bits(),
        });
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn map_word_to_range(word: u32, low: f32, high: f32) -> f32 {
    let unit = (word >> 8) as f32 * UNIT_F32_SCALE;
    let span = high - low;
    let scaled = span * unit;
    let value = low + scaled;
    value.clamp(low, high)
}
