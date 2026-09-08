let expire: (() => void) | undefined;

// Arm only after authentication, never while submitting the login form.
export function onSessionExpired(handler: () => void): void {
  expire = handler;
}

export function checkSessionResponse(status: number): void {
  if (status !== 401 || !expire) return;
  const handler = expire;
  expire = undefined;
  handler();
}
