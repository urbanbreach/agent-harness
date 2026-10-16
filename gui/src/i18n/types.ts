/** A complete translation of an English catalog: every key, no extras. */
export type Translations<T> = { readonly [K in keyof T]: string };
