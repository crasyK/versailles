export function listen(event, handler) {
  return window.versailles.listen(event, (payload) => handler({ payload }));
}
