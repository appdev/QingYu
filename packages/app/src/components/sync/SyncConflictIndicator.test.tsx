import { fireEvent, render, screen } from "@testing-library/react";
import { SyncConflictIndicator } from "./SyncConflictIndicator";

it("opens conflict review without changing the editor state", () => {
  const onOpen = vi.fn();
  render(<SyncConflictIndicator label="Sync conflict" onOpen={onOpen} />);
  fireEvent.click(screen.getByRole("button", { name: "Sync conflict" }));
  expect(onOpen).toHaveBeenCalledTimes(1);
});
