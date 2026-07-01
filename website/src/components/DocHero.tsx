import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';

export default function DocHero(): JSX.Element {
  const logoUrl = useBaseUrl('/img/logo.png');

  return (
    <div className="bunny-doc-hero">
      <div className="bunny-doc-hero__layout">
        <div className="bunny-doc-hero__content">
          <p className="bunny-doc-hero__eyebrow">Documentation</p>
          <h2 className="bunny-doc-hero__title">Coding in multiplayer mode with AI agents and your team</h2>
          <p className="bunny-doc-hero__lead">
            Bunny is a self-hosted workspace where your team and your AI agents build on the same
            remote machine: shared shells (SSH access), live previews (streamed browser windows), and teams chats (Discord, Slack, Teams) in one place. It's like a remote desktop, but for your team and your AI agents, repecting your governance (policies, access control, etc.).
          </p>
          <div className="bunny-doc-hero__actions">
            <Link className="bunny-btn bunny-btn--primary" to="/getting-started/choose-your-path">
              Get started
            </Link>
            <Link className="bunny-btn bunny-btn--secondary" to="/getting-started/first-run">
              First-run checklist
            </Link>
            <Link className="bunny-btn bunny-btn--secondary" to="/team-chats/discord/setup">
              Connect Discord
            </Link>
          </div>
        </div>
        <div className="bunny-doc-hero__visual">
          <img src={logoUrl} alt="bunny" className="bunny-doc-hero__logo" width={280} height={252} />
        </div>
      </div>
    </div>
  );
}
