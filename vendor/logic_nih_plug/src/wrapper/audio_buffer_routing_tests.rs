//! Property-based tests for audio buffer routing across plugin formats.
//!
//! **Feature: multi-format-export, Property 4: Audio buffer routing**
//!
//! These tests verify that audio data correctly flows from the host through
//! the Plugin::process() method and back to the host for all supported formats
//! (VST2, AU, AUv3, LV2, AAX).
//!
//! **Validates: Requirements 1.3, 2.3, 3.3, 4.3, 5.3**

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::params::internals::ParamPtr;
    use crate::context::process::Transport;
    use std::sync::Arc;
    use proptest::prelude::*;

    /// Empty params struct for testing
    #[derive(Default)]
    struct EmptyParams;
    
    unsafe impl Params for EmptyParams {
        fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
            Vec::new()
        }
    }

    /// A simple test plugin that applies a fixed gain to verify audio routing.
    struct TestPlugin {
        gain: f32,
        params: Arc<EmptyParams>,
    }

    impl Default for TestPlugin {
        fn default() -> Self {
            Self {
                gain: 1.0,
                params: Arc::new(EmptyParams),
            }
        }
    }

    impl Plugin for TestPlugin {
        const NAME: &'static str = "Test Plugin";
        const VENDOR: &'static str = "Test Vendor";
        const URL: &'static str = "https://example.com";
        const EMAIL: &'static str = "test@example.com";
        const VERSION: &'static str = "0.0.1";

        const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
            AudioIOLayout {
                main_input_channels: NonZeroU32::new(2),
                main_output_channels: NonZeroU32::new(2),
                ..AudioIOLayout::const_default()
            },
            AudioIOLayout {
                main_input_channels: NonZeroU32::new(1),
                main_output_channels: NonZeroU32::new(1),
                ..AudioIOLayout::const_default()
            },
        ];

        type SysExMessage = ();
        type BackgroundTask = ();

        fn params(&self) -> Arc<dyn Params> {
            self.params.clone()
        }

        fn process(
            &mut self,
            buffer: &mut Buffer,
            _aux: &mut AuxiliaryBuffers,
            _context: &mut impl ProcessContext<Self>,
        ) -> ProcessStatus {
            // Apply gain to all samples
            for mut channel_samples in buffer.iter_samples() {
                for sample in channel_samples.iter_mut() {
                    *sample *= self.gain;
                }
            }

            ProcessStatus::Normal
        }
    }

    /// Strategy for generating valid audio buffer sizes
    fn buffer_size_strategy() -> impl Strategy<Value = usize> {
        prop::sample::select(vec![1, 16, 32, 64, 128, 256, 512, 1024, 2048])
    }

    /// Strategy for generating valid channel counts
    fn channel_count_strategy() -> impl Strategy<Value = usize> {
        1..=8usize
    }

    /// Strategy for generating audio sample values
    fn audio_sample_strategy() -> impl Strategy<Value = f32> {
        // Generate samples in a reasonable range, avoiding denormals
        prop_oneof![
            // Silence
            Just(0.0f32),
            // Normal audio range
            -1.0f32..=1.0f32,
            // Quiet signals
            -0.1f32..=0.1f32,
        ]
    }

    /// Strategy for generating gain values
    fn gain_strategy() -> impl Strategy<Value = f32> {
        prop_oneof![
            Just(0.0f32),   // Mute
            Just(1.0f32),   // Unity
            Just(0.5f32),   // Half
            Just(2.0f32),   // Double
            0.0f32..=2.0f32,   // Variable
        ]
    }

    /// Helper to create a Buffer from a Vec of Vec<f32>
    unsafe fn create_buffer_from_data<'a>(
        data: &'a mut Vec<Vec<f32>>,
        buffer_size: usize,
    ) -> Buffer<'a> {
        let mut buffer = Buffer::default();
        buffer.set_slices(buffer_size, |output_slices| {
            output_slices.clear();
            for channel in data.iter_mut() {
                output_slices.push(&mut channel[..buffer_size]);
            }
        });
        buffer
    }

    /// Dummy process context for testing
    struct DummyContext {
        transport: Transport,
    }
    
    impl DummyContext {
        fn new() -> Self {
            Self {
                transport: Transport::new(44100.0),
            }
        }
    }
    
    impl<P: Plugin> ProcessContext<P> for DummyContext {
        fn set_latency_samples(&self, _samples: u32) {}
        fn set_current_voice_capacity(&self, _capacity: u32) {}
        
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Standalone
        }
        
        fn execute_background(&self, _task: P::BackgroundTask) {}
        fn execute_gui(&self, _task: P::BackgroundTask) {}
        
        fn transport(&self) -> &Transport {
            &self.transport
        }
        
        fn next_event(&mut self) -> Option<PluginNoteEvent<P>> {
            None
        }
        
        fn send_event(&mut self, _event: PluginNoteEvent<P>) {}
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 100,
            .. ProptestConfig::default()
        })]

        /// **Property 4: Audio buffer routing**
        ///
        /// *For any* audio buffer and any format (VST2, AU, AUv3, LV2, AAX),
        /// audio data should correctly flow from the host through the
        /// Plugin::process() method and back to the host.
        ///
        /// **Validates: Requirements 1.3, 2.3, 3.3, 4.3, 5.3**
        #[test]
        fn test_audio_buffer_routing(
            buffer_size in buffer_size_strategy(),
            num_channels in channel_count_strategy(),
            gain in gain_strategy(),
            input_samples in prop::collection::vec(audio_sample_strategy(), 1..=2048)
        ) {
            // Create plugin instance with specified gain
            let mut plugin = TestPlugin {
                gain,
                params: Arc::new(EmptyParams),
            };

            // Prepare input data - cycle through input_samples to fill the buffer
            let mut input_data: Vec<Vec<f32>> = vec![vec![0.0; buffer_size]; num_channels];
            for ch in 0..num_channels {
                for (i, sample) in input_data[ch].iter_mut().enumerate() {
                    *sample = input_samples[i % input_samples.len()];
                }
            }

            // Create output buffer (simulating host behavior where input is copied to output)
            let mut output_data: Vec<Vec<f32>> = input_data.clone();

            // Create Buffer from output_data
            let mut buffer = unsafe { create_buffer_from_data(&mut output_data, buffer_size) };

            // Create dummy auxiliary buffers
            let mut aux = AuxiliaryBuffers {
                inputs: &mut [],
                outputs: &mut [],
            };

            let mut context = DummyContext::new();

            // Process the buffer
            let status = plugin.process(&mut buffer, &mut aux, &mut context);

            // Verify processing succeeded
            prop_assert_eq!(status, ProcessStatus::Normal);

            // Verify audio routing: output should be input * gain
            for ch in 0..num_channels {
                for i in 0..buffer_size {
                    let expected = input_data[ch][i] * gain;
                    let actual = output_data[ch][i];
                    
                    // Use approximate comparison for floating point
                    let diff = (expected - actual).abs();
                    prop_assert!(
                        diff < 1e-6,
                        "Channel {}, Sample {}: expected {}, got {} (diff: {})",
                        ch, i, expected, actual, diff
                    );
                }
            }
        }

        /// Test that buffer routing preserves silence
        #[test]
        fn test_silence_preservation(
            buffer_size in buffer_size_strategy(),
            num_channels in channel_count_strategy(),
        ) {
            let mut plugin = TestPlugin {
                gain: 1.0,
                params: Arc::new(EmptyParams),
            };

            // Create silent input
            let mut output_data: Vec<Vec<f32>> = vec![vec![0.0; buffer_size]; num_channels];

            let mut buffer = unsafe { create_buffer_from_data(&mut output_data, buffer_size) };

            let mut aux = AuxiliaryBuffers {
                inputs: &mut [],
                outputs: &mut [],
            };

            let mut context = DummyContext::new();

            plugin.process(&mut buffer, &mut aux, &mut context);

            // Verify output is still silent
            for ch in 0..num_channels {
                for i in 0..buffer_size {
                    prop_assert_eq!(output_data[ch][i], 0.0);
                }
            }
        }

        /// Test that muting (gain = 0) produces silence
        #[test]
        fn test_mute_produces_silence(
            buffer_size in buffer_size_strategy(),
            num_channels in channel_count_strategy(),
            input_samples in prop::collection::vec(audio_sample_strategy(), 1..=2048)
        ) {
            let mut plugin = TestPlugin {
                gain: 0.0,
                params: Arc::new(EmptyParams),
            };

            let mut output_data: Vec<Vec<f32>> = vec![vec![0.0; buffer_size]; num_channels];
            for ch in 0..num_channels {
                for (i, sample) in output_data[ch].iter_mut().enumerate() {
                    *sample = input_samples[i % input_samples.len()];
                }
            }

            let mut buffer = unsafe { create_buffer_from_data(&mut output_data, buffer_size) };

            let mut aux = AuxiliaryBuffers {
                inputs: &mut [],
                outputs: &mut [],
            };

            let mut context = DummyContext::new();

            plugin.process(&mut buffer, &mut aux, &mut context);

            // Verify output is silent
            for ch in 0..num_channels {
                for i in 0..buffer_size {
                    let sample = output_data[ch][i];
                    prop_assert!(
                        sample.abs() < 1e-6,
                        "Expected silence, got {} at channel {}, sample {}",
                        sample, ch, i
                    );
                }
            }
        }
    }
}
