import { lazy, Suspense, useEffect } from 'react';
import HomePage from './components/HomePage';
import LoginPage from './components/LoginPage';
import { useAuth } from './store/auth';

const SessionWorkspace = lazy(() => import('./components/SessionWorkspace'));
const SecretsPage = lazy(() => import('./components/SecretsPage'));

function parseSessionFromPath(): string | null {
  const m = location.pathname.match(/^\/s\/([^/]+)/);
  return m ? m[1] : null;
}

export default function App() {
  const { user, loading, check } = useAuth();
  const sessionId = parseSessionFromPath();

  useEffect(() => {
    check();
  }, [check]);

  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center text-bunny-muted">
        Loading…
      </div>
    );
  }

  if (!user) {
    return <LoginPage />;
  }

  if (sessionId) {
    return (
      <Suspense
        fallback={
          <div className="min-h-screen flex items-center justify-center text-bunny-muted">
            Loading session…
          </div>
        }
      >
        <SessionWorkspace sessionId={sessionId} />
      </Suspense>
    );
  }

  if (location.pathname === '/secrets') {
    if (!user.isOwner) {
      location.href = '/';
      return null;
    }
    return (
      <Suspense
        fallback={
          <div className="min-h-screen flex items-center justify-center text-bunny-muted">
            Loading…
          </div>
        }
      >
        <SecretsPage email={user.email} />
      </Suspense>
    );
  }

  return <HomePage email={user.email} isOwner={user.isOwner} />;
}
