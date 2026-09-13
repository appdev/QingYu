import type {MarkdownAnnotations, MarkdownAnnotation} from "./types";
import {emptyAnnotations, parseAnnotations} from "./types";
import {reanchor} from "./anchors";

export class MarkdownAnnotationController {
    private document: MarkdownAnnotations;
    private persisted: readonly MarkdownAnnotation[];

    constructor(data: MarkdownAnnotations | undefined, text: string) {
        this.document = data ? parseAnnotations(data) : emptyAnnotations();
        this.persisted = this.document.records.map((record) => reanchor(record, text));
    }

    get initial() { return this.persisted; }

    snapshot(records: readonly MarkdownAnnotation[]): MarkdownAnnotations | undefined {
        if (!records.length && this.document.revision === 0) return undefined;
        return {...this.document, records: records.map((record) => ({...record}))};
    }

    saved(snapshot: MarkdownAnnotations | undefined, response: MarkdownAnnotations | undefined) {
        if (!snapshot) return;
        if (!response || response.documentId !== snapshot.documentId || response.revision !== snapshot.revision + 1) {
            throw new Error("ANNOTATION_SAVE_NOT_ACKNOWLEDGED");
        }
        this.document = parseAnnotations(response);
    }
}
