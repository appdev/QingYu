export class StatusbarOwnership {
    private owner: unknown;
    private revision = 0;

    claim(owner: unknown) {
        this.owner = owner;
        this.revision += 1;
        return this.revision;
    }

    owns(owner: unknown, revision: number) {
        return this.owner === owner && this.revision === revision;
    }

    release(owner: unknown) {
        if (this.owner !== owner) return false;
        this.owner = undefined;
        this.revision += 1;
        return true;
    }

    reset() {
        this.owner = undefined;
        this.revision += 1;
    }
}

export const isMarkdownStatisticsOwnerEligible = (
    view: {hasFocus: boolean} | undefined,
    element: Element | undefined,
) => Boolean(view?.hasFocus && element && !element.closest(".fn__none"));
