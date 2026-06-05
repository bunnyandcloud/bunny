import { lazy, Suspense, useEffect } from 'react';
import HomePage from './components/HomePage';
import InviteAcceptPage from './components/InviteAcceptPage';
import LoginPage from './components/LoginPage';
import LanguageSelect from './components/LanguageSelect';
import { useT } from './i18n';
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

function LoadingScreen({ message }: { message: string }) {
  return (
    <div className="min-h-screen flex items-center justify-center text-bunny-muted">
      {message}
    </div>
  );
}

export default function App() {
  const { user, loading, check, logout } = useAuth();
  const tr = useT();
  const sessionId = parseSessionFromPath();
  const watchToken = parseWatchToken();
  const inviteToken = new URLSearchParams(location.search).get('invite');
  const inviteEmail = new URLSearchParams(location.search).get('email');

  useEffect(() => {
    check();
  }, [check]);

  if (loading) {
    return <LoadingScreen message={tr('web.common.loading')} />;
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
        fallback={<LoadingScreen message={tr('web.common.loadingWatch')} />}
      >
        <WatchPage token={watchToken} />
      </Suspense>
    );
  }

  if (!user) {
    return (
      <div className="min-h-screen relative">
        <div className="absolute top-4 right-4 z-10">
          <LanguageSelect />
        </div>
        <LoginPage />
      </div>
    );
  }

  if (sessionId) {
    return (
      <Suspense
        fallback={<LoadingScreen message={tr('web.common.loadingSession')} />}
      >
        <SessionWorkspace sessionId={sessionId} />
      </Suspense>
    );
  }

  if (location.pathname === '/security') {
    return (
      <Suspense
        fallback={<LoadingScreen message={tr('web.common.loading')} />}
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
        fallback={<LoadingScreen message={tr('web.common.loading')} />}
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
        fallback={<LoadingScreen message={tr('web.common.loading')} />}
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
