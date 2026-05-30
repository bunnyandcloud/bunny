import { lazy, Suspense, useEffect } from 'react';
import HomePage from './components/HomePage';
import InviteAcceptPage from './components/InviteAcceptPage';
import LoginPage from './components/LoginPage';
import { useAuth } from './store/auth';

const SessionWorkspace = lazy(() => import('./components/SessionWorkspace'));
const SecretsPage = lazy(() => import('./components/SecretsPage'));
const SecurityPage = lazy(() => import('./components/SecurityPage'));
const TeamPage = lazy(() => import('./components/TeamPage'));
const WatchPage = lazy(() => import('./components/WatchPage'));

function parseSessionFromPath(): string | null {
  const m = location.pathname.match(/^\/s\/([^/]+)/);
  return m ? m[1] : null;
}

function parseWatchToken(): string | null {
  const m = location.pathname.match(/^\/watch\/([^/]+)/);
  return m ? m[1] : null;
}

export default function App() {
  const { user, loading, check, logout } = useAuth();
  const sessionId = parseSessionFromPath();
  const watchToken = parseWatchToken();
  const inviteToken = new URLSearchParams(location.search).get('invite');
  const inviteEmail = new URLSearchParams(location.search).get('email');

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

  if (inviteToken && user) {
    return (
      <InviteAcceptPage
        currentEmail={user.email}
        inviteEmail={inviteEmail}
        onSignOut={logout}
      />
    );
  }

  if (watchToken) {
    return (
      <Suspense
        fallback={
          <div className="min-h-screen flex items-center justify-center text-bunny-muted">
            Loading watch…
          </div>
        }
      >
        <WatchPage token={watchToken} />
      </Suspense>
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

  if (location.pathname === '/security') {
    return (
      <Suspense
        fallback={
          <div className="min-h-screen flex items-center justify-center text-bunny-muted">
            Loading…
          </div>
        }
      >
        <SecurityPage email={user.email} />
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

  if (location.pathname === '/team') {
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
        <TeamPage email={user.email} />
      </Suspense>
    );
  }

  return (
    <HomePage
      email={user.email}
      isOwner={user.isOwner}
      canCreateSessions={user.canCreateSessions}
    />
  );
}
