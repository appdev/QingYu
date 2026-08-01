export const SERVER_PASSWORD_MAX_LENGTH = 1024;
export const SERVER_PASSWORD_PATTERN = "[!-~]+";

const serverPasswordPattern = /^[!-~]+$/u;

export function isValidServerPassword(value: string) {
  return value.length <= SERVER_PASSWORD_MAX_LENGTH && serverPasswordPattern.test(value);
}
