import AppKit
import Testing

@testable import JEVision

struct IndicatorStateTests {
  @Test func nothingShowsWhenNothingRunsOrRecords() {
    #expect(IndicatorState(run: nil, isRecording: false, recordingPaused: false) == nil)
    #expect(IndicatorState(run: .finished(.cancelled), isRecording: false, recordingPaused: false) == nil)
  }

  @Test func runAndRecordingStates() {
    #expect(IndicatorState(run: .running, isRecording: false, recordingPaused: false) == .running)
    #expect(IndicatorState(run: .paused(.userInput), isRecording: false, recordingPaused: false) == .paused)
    #expect(IndicatorState(run: nil, isRecording: true, recordingPaused: false) == .recording)
    #expect(IndicatorState(run: nil, isRecording: true, recordingPaused: true) == .recordingPaused)
  }

  @Test func colorsAndPulse() {
    #expect(IndicatorState.running.color == .systemBlue)
    #expect(IndicatorState.recording.color == .systemRed)
    #expect(IndicatorState.paused.color == .systemOrange)
    #expect(IndicatorState.recordingPaused.color == .systemOrange)
    #expect(IndicatorState.running.pulses && IndicatorState.recording.pulses)
    #expect(!IndicatorState.paused.pulses && !IndicatorState.recordingPaused.pulses)
  }
}
