import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';

export default function DocHero(): JSX.Element {
  const logoUrl = useBaseUrl('/img/logo.png');

  return (
    <div className="bunny-doc-hero">
      <div className="bunny-doc-hero__layout">
        <div className="bunny-doc-hero__content">
          <p className="bunny-doc-hero__eyebrow">Documentation</p>
          <h2 className="bunny-doc-hero__title">Build together on a shared remote environment</h2>
          <p className="bunny-doc-hero__lead">
            Install bunny, configure your server, and connect Discord — same vision as{' '}
            <a href="https://bunnyandcloud.com" target="_blank" rel="noopener noreferrer">
              bunnyandcloud.com
            </a>
            .
          </p>
          <div className="bunny-doc-hero__actions">
            <Link className="bunny-btn bunny-btn--primary" to="/getting-started/choose-your-path">
              Choose your path
            </Link>
            <Link className="bunny-btn bunny-btn--secondary" to="/getting-started/configure-server">
              Configure the server
            </Link>
            <a
              className="bunny-btn bunny-btn--secondary"
              href="https://bunnyandcloud.com"
              target="_blank"
              rel="noopener noreferrer">
              Landing page
            </a>
          </div>
        </div>
        <div className="bunny-doc-hero__visual">
          <img src={logoUrl} alt="bunny" className="bunny-doc-hero__logo" width={280} height={252} />
        </div>
      </div>
    </div>
  );
}
