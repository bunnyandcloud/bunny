import { useEffect } from 'react';
import HomePage from './components/HomePage';
import LoginPage from './components/LoginPage';
import SessionWorkspace from './components/SessionWorkspace';
import { useAuth } from './store/auth';

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

  if (location.pathname === '/login' || (!user && !loading)) {
    return <LoginPage />;
  }

  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center text-bunny-muted">
        Loading…
      </div>
    );
  }

  if (!user) {
    location.href = `/login?next=${encodeURIComponent(location.pathname)}`;
    return null;
  }

  if (sessionId) {
    return <SessionWorkspace sessionId={sessionId} />;
  }

  return <HomePage email={user.email} />;
}
