// The management routes use HTTP Basic, but the browser never sees a native
// dialog: a 401 becomes a login form, and the action that hit it is retried.

import { AuthRequired, clearCredentials, setCredentials, verifyCredentials } from './api';

export const authPrompt = $state<{ open: boolean; error: string }>({ open: false, error: '' });

let waiting: { resolve: () => void; reject: (e: Error) => void }[] = [];

/** Run `fn`; if it needs credentials, ask for them and run it once more. */
export async function withAuth<T>(fn: () => Promise<T>): Promise<T> {
  try {
    return await fn();
  } catch (e) {
    if (!(e instanceof AuthRequired)) throw e;
  }
  await new Promise<void>((resolve, reject) => {
    waiting.push({ resolve, reject });
    authPrompt.open = true;
  });
  return fn();
}

export async function submitLogin(user: string, password: string): Promise<void> {
  setCredentials(user, password);
  try {
    if (!(await verifyCredentials())) {
      clearCredentials();
      authPrompt.error = 'Wrong username or password.';
      return;
    }
  } catch (e) {
    clearCredentials();
    authPrompt.error = e instanceof Error ? e.message : String(e);
    return;
  }
  authPrompt.open = false;
  authPrompt.error = '';
  const w = waiting;
  waiting = [];
  w.forEach((x) => x.resolve());
}

export function cancelLogin(): void {
  authPrompt.open = false;
  authPrompt.error = '';
  const w = waiting;
  waiting = [];
  w.forEach((x) => x.reject(new Error('sign-in cancelled')));
}
