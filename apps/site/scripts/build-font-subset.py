from pathlib import Path
import sys

from fontTools import subset
from fontTools.ttLib import TTFont


source = Path(sys.argv[1])
glyph_file = Path(sys.argv[2])
output = Path(sys.argv[3])

font = TTFont(source)
options = subset.Options()
options.layout_features = ["*"]
subsetter = subset.Subsetter(options=options)
subsetter.populate(text=glyph_file.read_text(encoding="utf-8"))
subsetter.subset(font)

names = font["name"]
for name_id in [1, 2, 3, 4, 6, 16, 17]:
    names.names = [record for record in names.names if record.nameID != name_id]
for platform_id, encoding_id, language_id in [(3, 1, 0x409), (1, 0, 0)]:
    names.setName("QingYu WenKai Subset", 1, platform_id, encoding_id, language_id)
    names.setName("Regular", 2, platform_id, encoding_id, language_id)
    names.setName("QingYuWenKaiSubset-Regular", 3, platform_id, encoding_id, language_id)
    names.setName("QingYu WenKai Subset Regular", 4, platform_id, encoding_id, language_id)
    names.setName("QingYuWenKaiSubset-Regular", 6, platform_id, encoding_id, language_id)
    names.setName("QingYu WenKai Subset", 16, platform_id, encoding_id, language_id)
    names.setName("Regular", 17, platform_id, encoding_id, language_id)

output.parent.mkdir(parents=True, exist_ok=True)
font.flavor = "woff2"
font.save(output)
