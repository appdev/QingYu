import {BUNDLED_CODE_LANGUAGES} from "./codeLanguages.generated";

export const CODE_LANGUAGE_ALIASES = ["js", "ts", "html", "toml", "c#", "bat"];

export const getCodeLanguages = () => CODE_LANGUAGE_ALIASES
    .concat(BUNDLED_CODE_LANGUAGES, window.hljs?.listLanguages() ?? [])
    .filter((language, index, languages) => languages.indexOf(language) === index)
    .sort();
