export class ExternalEditorRegistry<T extends object> {
    private owners = new Map<string, T>();

    public claim(capabilityId: string, owner: T) {
        const existing = this.owners.get(capabilityId);
        if (existing && existing !== owner) return existing;
        this.owners.set(capabilityId, owner);
        return undefined;
    }

    public release(capabilityId: string, owner: T) {
        if (this.owners.get(capabilityId) !== owner) return false;
        this.owners.delete(capabilityId);
        return true;
    }
}
