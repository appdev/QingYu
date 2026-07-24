import { fireEvent, render, screen } from "@testing-library/react";
import { NotesWorkspaceSettings } from "./NotesWorkspaceSettings";
import { t } from "@markra/shared";

describe("NotesWorkspaceSettings", () => {
  it("shows the configured path read-only and explains that changing it moves no files", () => {
    const onChoose = vi.fn();
    const onResetOnboarding = vi.fn();

    render(
      <NotesWorkspaceSettings
        root="/Users/ying/Notes"
        status="ready"
        translate={(key) => t("en", key)}
        onChoose={onChoose}
        onResetOnboarding={onResetOnboarding}
      />
    );

    expect(screen.getByText("/Users/ying/Notes")).toBeVisible();
    expect(screen.getByText(/does not move any files/iu)).toBeVisible();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Switch Notebook Directory" }));
    fireEvent.click(screen.getByRole("button", { name: "Show onboarding next launch" }));
    expect(onChoose).toHaveBeenCalledOnce();
    expect(onResetOnboarding).toHaveBeenCalledOnce();
  });

  it("shows an unavailable recovery state without exposing an editable path", () => {
    render(
      <NotesWorkspaceSettings
        root={null}
        status="recovery"
        translate={(key) => t("en", key)}
        onChoose={vi.fn()}
        onResetOnboarding={vi.fn()}
      />
    );

    expect(screen.getByText("The configured notes folder is unavailable.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Switch Notebook Directory" })).toBeVisible();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });
});
