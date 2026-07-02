import React from 'react';
import clsx from 'clsx';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import {useThemeConfig} from '@docusaurus/theme-common';
import Logo from '@theme/Logo';
import CollapseButton from '@theme/DocSidebar/Desktop/CollapseButton';
import Content from '@theme/DocSidebar/Desktop/Content';
import type {Props} from '@theme/DocSidebar/Desktop';

import styles from './styles.module.css';

function DocSidebarDesktop({path, sidebar, onCollapse, isHidden}: Props) {
  const {
    docs: {
      sidebar: {hideable},
    },
  } = useThemeConfig();
  const {
    siteConfig: {customFields},
  } = useDocusaurusContext();
  const docVersion =
    typeof customFields?.docVersion === 'string' ? customFields.docVersion : null;

  return (
    <div className={clsx(styles.sidebar, isHidden && styles.sidebarHidden)}>
      <div className={styles.sidebarHeader}>
        <Logo tabIndex={-1} className={styles.sidebarLogo} imageClassName="bunny-sidebar-logo" />
      </div>
      <Content path={path} sidebar={sidebar} className={styles.sidebarContent} />
      {docVersion ? (
        <div className={styles.sidebarVersionFooter}>
          <span className={styles.sidebarVersionLabel}>Latest version :</span>
          <span className={styles.sidebarVersion}>{docVersion}</span>
        </div>
      ) : null}
      {hideable && <CollapseButton onClick={onCollapse} />}
    </div>
  );
}

export default React.memo(DocSidebarDesktop);
