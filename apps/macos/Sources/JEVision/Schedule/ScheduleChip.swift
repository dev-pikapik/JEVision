import SwiftUI

/// The next scheduled run, at the foot of the sidebar; opens the schedules.
struct ScheduleChip: View {
  @Environment(ProjectStore.self) private var project

  var body: some View {
    let schedules = project.schedules
    let next = schedules.all.compactMap { ProjectSchedules.nextRun(of: $0) }.min()
    let needsAttention = schedules.problem != nil || (!schedules.all.isEmpty && !schedules.isOn)
    let text =
      if schedules.problem != nil {
        String(localized: "Schedules can’t be read")
      } else if schedules.all.isEmpty {
        String(localized: "No Schedules")
      } else if !schedules.isOn {
        String(localized: "Schedules are off on this Mac")
      } else if let next {
        String(localized: "Next run \(ScheduleText.moment(next))")
      } else {
        String(localized: "Schedules")
      }
    Button {
      schedules.isShowingSheet = true
    } label: {
      HStack(spacing: 6) {
        Image(systemName: needsAttention ? "exclamationmark.triangle.fill" : "calendar.badge.clock")
          .foregroundStyle(needsAttention ? Color.orange : Color.secondary)
        Text(verbatim: text)
          .lineLimit(1)
          .truncationMode(.middle)
        Spacer(minLength: 0)
      }
      .font(.callout)
      .foregroundStyle(.secondary)
      .padding(.horizontal, 16)
      .padding(.vertical, 8)
      .contentShape(.rect)
    }
    .buttonStyle(.plain)
    .help(Text("Run tests on their own at set times"))
    .accessibilityLabel(Text("Schedules"))
    .accessibilityValue(Text(verbatim: text))
  }
}
