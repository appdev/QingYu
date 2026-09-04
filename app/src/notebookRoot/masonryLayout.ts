export interface NotebookRootMasonryInput {
    containerWidth: number;
    ratios: number[];
    gap?: number;
    paddingTop?: number;
    paddingRight?: number;
    paddingBottom?: number;
    paddingLeft?: number;
}

export interface NotebookRootMasonryPlacement {
    left: number;
    top: number;
    width: number;
    height: number;
}

export interface NotebookRootMasonryLayout {
    columnCount: number;
    cardWidth: number;
    height: number;
    placements: NotebookRootMasonryPlacement[];
}

const finiteDimension = (value: number, fallback = 0) => Number.isFinite(value) ? Math.max(0, value) : fallback;

export const notebookRootMasonryColumnCount = (width: number) => {
    const normalizedWidth = finiteDimension(width);
    if (normalizedWidth <= 451) return 1;
    if (normalizedWidth <= 671) return 2;
    if (normalizedWidth <= 891) return 3;
    if (normalizedWidth <= 1111) return 4;
    return 5;
};

export const calculateNotebookRootMasonryLayout = (input: NotebookRootMasonryInput): NotebookRootMasonryLayout => {
    const containerWidth = finiteDimension(input.containerWidth);
    const gap = finiteDimension(input.gap ?? 20);
    const paddingTop = finiteDimension(input.paddingTop ?? 28);
    const paddingRight = finiteDimension(input.paddingRight ?? 16);
    const paddingBottom = finiteDimension(input.paddingBottom ?? 48);
    const paddingLeft = finiteDimension(input.paddingLeft ?? 16);
    const columnCount = notebookRootMasonryColumnCount(containerWidth);
    const innerWidth = Math.max(0, containerWidth - paddingLeft - paddingRight);
    const cardWidth = Math.max(0, (innerWidth - gap * (columnCount - 1)) / columnCount);
    const columnHeights = Array<number>(columnCount).fill(paddingTop);
    const placements = input.ratios.map((rawRatio) => {
        const ratio = Number.isFinite(rawRatio) && rawRatio > 0 ? rawRatio : 1;
        const shortest = Math.min(...columnHeights);
        const column = columnHeights.indexOf(shortest);
        const placement = {
            left: paddingLeft + column * (cardWidth + gap),
            top: shortest,
            width: cardWidth,
            height: cardWidth * ratio,
        };
        columnHeights[column] = placement.top + placement.height + gap;
        return placement;
    });
    const height = placements.length === 0 ? 0 : Math.max(...columnHeights) - gap + paddingBottom;
    return {columnCount, cardWidth, height, placements};
};
