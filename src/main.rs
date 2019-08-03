extern crate clap;
extern crate standard_midi_file;
extern crate synthesizer;

use clap::{App, Arg};
use standard_midi_file::header::TimeScale;
use standard_midi_file::track::event::Event;
use standard_midi_file::SMF;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use synthesizer::frequency_lookup::MIDIFrequencyLookup;
use synthesizer::helper::SequenceHelper;
use synthesizer::instrument::Instrument;
use synthesizer::key_generator::{
    SawtoothWaveGenerator, SquareWaveGenerator, TriangleWaveGenerator,
};
use synthesizer::pcm::PCMParameters;
use synthesizer::util::Volume;
use synthesizer::wave::{SampleType, Wave};
use synthesizer::Synthesizer;

struct TempoHelper {
    data: Vec<(u32, u32)>,
}

impl TempoHelper {
    fn new() -> TempoHelper {
        TempoHelper { data: Vec::new() }
    }
    fn new_tempo(&mut self, tick: u32, tempo: u32) {
        self.data.push((tick, tempo));
    }
    fn get_tempo(&mut self, tick: u32) -> u32 {
        self.data.sort();
        self.data.reverse();
        for (at_tick, tempo) in &self.data {
            if tick >= *at_tick {
                return *tempo;
            }
        }
        500000
    }
}

fn calc_time(ticks: u32, tempo: u32, ticks_per_quarter_note: u16) -> f64 {
    (f64::from(tempo) / f64::from(ticks_per_quarter_note)) * f64::from(ticks) * 10f64.powi(-6)
}

fn main() {
    let matches = App::new("MIDI Synthesizer")
        .version("0.1")
        .author("Marime Gui")
        .about("Synthesizes a MIDI File to q Square Wave WAV File")
        .arg(
            Arg::with_name("INPUT")
                .help("Input .mid file")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("OUTPUT")
                .help("Output .wav file")
                .required(true)
                .index(2),
        )
        .arg(
            Arg::with_name("FUNCTION")
                .help("Chooses the sound generator function. Possible values are 'square', 'triangles', 'sawtooth'.")
                .required(false)
                .index(3),
        )
        .get_matches();

    let input_str = matches.value_of("INPUT").unwrap();
    let output_str = matches.value_of("OUTPUT").unwrap();
    let input_path = Path::new(input_str);
    let output_path = Path::new(output_str);

    // Open the MIDI File
    let smf = SMF::import(&mut BufReader::new(File::open(input_path).unwrap())).unwrap();
    let tpqn = match smf.header.time_division {
        TimeScale::TicksPerQuarterNote(t) => t,
        TimeScale::SMPTECompatible(_, _) => unimplemented!(),
    };

    // Create Tempo Helper
    let mut tempo_helper = TempoHelper::new();

    // Find all tempos first
    for track in &smf.tracks {
        let mut at_tick = 0;
        for track_event in &track.track_events {
            at_tick += track_event.delta_time.value;
            match track_event.event {
                Event::Tempo(t) => tempo_helper.new_tempo(at_tick, t.value),
                _ => {}
            }
        }
    }

    // Create a Sequence Helper
    let mut seq_builder = SequenceHelper::new();

    // Go through everything
    for track in &smf.tracks {
        seq_builder.reset();
        let mut at_tick = 0;
        for track_event in &track.track_events {
            at_tick += track_event.delta_time.value;
            seq_builder.time_forward(calc_time(
                track_event.delta_time.value,
                tempo_helper.get_tempo(at_tick),
                tpqn,
            ));
            match track_event.event {
                Event::NoteOn(n) => {
                    if n.velocity > 0 {
                        seq_builder
                            .start_note(
                                usize::from(n.key),
                                0,
                                vec![Volume::new(f64::from(n.velocity) / 128f64).unwrap()],
                            )
                            .unwrap();
                    } else {
                        seq_builder.stop_note(usize::from(n.key), 0).unwrap();
                    }
                }
                Event::NoteOff(n) => seq_builder.stop_note(usize::from(n.key), 0).unwrap(),
                _ => {}
            }
        }
    }

    let mut inst = HashMap::with_capacity(1);
    inst.insert(
        0,
        Instrument {
            keys: HashMap::new(),
            key_gen: match matches.value_of("FUNCTION").unwrap_or("") {
                "triangle" => Box::new(TriangleWaveGenerator {}),
                "sawtooth" => Box::new(SawtoothWaveGenerator {}),
                _ => Box::new(SquareWaveGenerator {}),
            },
            loopable: false,
        },
    );

    // Create the Synth
    let mut synth = Synthesizer {
        seq: seq_builder.sequence,
        inst,
        f_lu: Box::new(MIDIFrequencyLookup {}),
        params: PCMParameters {
            sample_rate: 44100,
            nb_channels: 1,
        },
    };

    // Run the Synth
    let pcm = synth.run().unwrap();

    // Create the Wave file
    let wave = Wave {
        pcm,
        sample_type: SampleType::Signed16,
    };

    let mut writer = BufWriter::new(File::create(output_path).unwrap());

    wave.write(&mut writer).unwrap();
}
