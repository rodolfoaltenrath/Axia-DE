// Only compatibility consumers cross this bridge; Rust owns locale state.
export const localeBridge = { perform: null };
export function nativeLocale(value) {
  const reply = lord.locale.state(value);
  if (!reply || reply.error) throw new Error(reply?.error || 'locale.invalidRequest');
  return reply;
}
